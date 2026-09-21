use core::pin::Pin;

use embedded_io::Write as _;
use uuid::Uuid;

use crate::{
    ADDRESS_BOOK_ENTRIES, MAX_TCP_CONNECTION_CHANNEL_SIZE, MAX_TCP_CONNECTIONS, TCP_PORT,
    UDP_CHANNEL_SIZE,
    fs::{self, WriteBytes},
    http::{BAD_REQUEST, INDEX_HTML, INTERNAL_SERVER_ERROR, METHOD_NOT_ALLOWED, Method, OK},
    kinds::{AddressBook, JobLedger},
    log_error, log_info,
    lwip::{
        my_ipaddr,
        put::put_file,
        tcp::{TcpConnection, TcpListener},
        udp::UdpSocket,
    },
    tasks::{messages::Message, rng::generate_uuid_v7},
};

/// This task receives new tcp handlers and spawns
/// tasks to manage each one.
#[embassy_executor::task(pool_size = 2)]
pub async fn tcp_worker(
    tcp: &'static TcpListener<MAX_TCP_CONNECTIONS, MAX_TCP_CONNECTION_CHANNEL_SIZE>,
    udp: &'static UdpSocket<UDP_CHANNEL_SIZE>,
    address_book: &'static AddressBook<ADDRESS_BOOK_ENTRIES>,
    ledger: &'static JobLedger,
) {
    loop {
        tcp.with_connection(async |conn| {
            log_info!("New Connection Received");
            handle_conn(conn, udp, address_book, ledger).await
        })
        .await
    }
}

/// A task that handles TCP requests for the printer. There is only `GET /` and `PUT /` to
/// retrieve the submission and put files onto the network for processing.
pub async fn handle_conn<const N1: usize, const N2: usize, const N3: usize>(
    conn: Pin<&mut TcpConnection<N1>>,
    udp: &UdpSocket<N2>,
    address_book: &AddressBook<N3>,
    ledger: &JobLedger,
) {
    log_info!("Handling TCP Connection");
    let Some(pbuf) = conn.as_ref().receive().await else {
        log_error!("Handle Closed");
        return;
    };

    let mut iter = pbuf.into_iter();

    let Some(chunk) = iter.next() else {
        let _ = conn.response(BAD_REQUEST.as_bytes()).await;
        return;
    };

    let Some((start_line, headers, body)) = split_request(chunk) else {
        let _ = conn.response(BAD_REQUEST.as_bytes()).await;
        return;
    };

    let method = match check_start_line(start_line) {
        Ok(method) => method,
        Err(e) => {
            let _ = conn.response(e.as_bytes()).await;
            return;
        }
    };

    match method {
        Method::Get => {
            log_info!("/ GET request received");
            let _ = conn.response(INDEX_HTML.as_bytes()).await;
        }
        Method::Put => {
            log_info!("/ PUT request received");

            let info = check_put_header(headers);
            if !info.is_gcode
                || info.size.is_none()
                || info.size.is_some_and(|s| s == 0 || s > 1_000_000)
            {
                let _ = conn.response(BAD_REQUEST.as_bytes()).await;
                return;
            }

            let guid = match info.guid {
                Some(guid) => guid,
                None => generate_uuid_v7(),
            };
            let path = fs::make_path(&guid, true);

            let Ok(mut f) = fs::open(path.as_c_str(), WriteBytes) else {
                let _ = conn.response(INTERNAL_SERVER_ERROR.as_bytes()).await;
                return;
            };

            let mut content_length = info.size.unwrap();

            // Write bytes to file from current chunk
            let to_write = core::cmp::min(content_length, body.len());
            let _ = f.write(&body[..to_write]);
            content_length = content_length.saturating_sub(to_write);

            // Check the rest of the existing chain
            let mut more_packets_needed = true;
            for chunk in iter {
                log_info!("Chunk Length: {}", chunk.len());
                let to_write = core::cmp::min(content_length, chunk.len());
                let _ = f.write(&chunk[..to_write]);
                content_length = content_length.saturating_sub(to_write);
                if content_length == 0 {
                    more_packets_needed = false;
                    break;
                }
            }

            // Do we need more chains? If so, wait to digest them.
            if more_packets_needed {
                while content_length > 0 {
                    let Some(pbuf) = conn.as_ref().receive().await else {
                        log_error!("Handle Reset");
                        f.close();
                        fs::delete(&path);
                        return;
                    };
                    for chunk in pbuf.into_iter() {
                        log_info!("Chunk Length: {}", chunk.len());
                        let to_write = core::cmp::min(content_length, chunk.len());
                        let _ = f.write(&chunk[..to_write]);
                        content_length = content_length.saturating_sub(to_write);
                        if content_length == 0 {
                            break;
                        }
                    }
                }
            }

            f.close();
            let new_path = fs::make_path(&guid, false);
            // Rename from partial to full.
            fs::rname(&path, &new_path);
            let _ = conn.response(OK.as_bytes()).await;

            if info.guid.is_none() {
                // New file to the system so we alert everyone else
                append_to_ledger(guid, address_book, ledger, udp).await;
                distribute_file(guid, address_book).await;
            }
        }
    }
}

/// Takes the incoming TCP request and separates the start_line, headers and body
/// for further processing.
fn split_request(buf: &[u8]) -> Option<(&str, &str, &[u8])> {
    let delim = b"\r\n";
    let idx = buf.windows(delim.len()).position(|win| win == delim)?;
    let (start_line, rest) = buf.split_at(idx);
    let rest = &rest[2..];
    let delim = b"\r\n\r\n";
    let idx = rest.windows(delim.len()).position(|win| win == delim)?;
    let (headers, rest) = rest.split_at(idx);
    let body = &rest[4..];
    let Ok(start_line) = str::from_utf8(start_line) else {
        return None;
    };
    let Ok(headers) = str::from_utf8(headers) else {
        return None;
    };
    Some((start_line, headers, body))
}

/// Checks whether the `start_line` is a valid address that we respond to.
fn check_start_line(start_line: &str) -> Result<Method, &'static str> {
    let mut tokens = start_line.split(" ");
    let Some(method) = tokens.next() else {
        return Err(BAD_REQUEST);
    };
    let method = match method {
        "GET" => Method::Get,
        "PUT" => Method::Put,
        _ => return Err(METHOD_NOT_ALLOWED),
    };

    let Some(url) = tokens.next() else {
        return Err(BAD_REQUEST);
    };
    if url != "/" {
        return Err(BAD_REQUEST);
    }

    Ok(method)
}

/// The fields extracted from a PUT request's headers by
/// `check_put_header`.
#[derive(Debug)]
pub struct PutInfo {
    pub size: Option<usize>,
    pub guid: Option<Uuid>,
    pub is_gcode: bool,
}

/// Analyses the PUT header to ensure it features the information
/// we require to process the request.
//
// Checked 2026-09-21: browser fetch() does in fact send `Content-Type:
// text/x.gcode` for the bundled upload form despite no explicit header in
// assets/index.html's JS, so `is_gcode` below is not the issue - false
// alarm, not the cause of the NS_ERROR_NET_RESET failures (see the BUG
// note on `TcpConnection::Drop` in src/lwip/tcp.rs for that).
fn check_put_header(headers: &str) -> PutInfo {
    let mut info = PutInfo {
        size: None,
        guid: None,
        is_gcode: false,
    };

    for line in headers.lines() {
        let Some((key, val)) = line.split_once(':') else {
            // TODO: error out again?
            continue;
        };
        let key = key.trim();
        let val = val.trim();

        if key.eq_ignore_ascii_case("content-length") {
            info.size = val.parse::<usize>().ok();
        } else if key.eq_ignore_ascii_case("content-type") && val == "text/x.gcode" {
            info.is_gcode = true;
        } else if key.eq_ignore_ascii_case("guid") {
            info.guid = val.parse::<Uuid>().ok();
        }
    }

    info
}

/// If we own the ledger, adds the new job to it; otherwise alerts the
/// network to the new job, unless we're the only machine around.
async fn append_to_ledger<const N1: usize, const N2: usize>(
    guid: Uuid,
    address_book: &AddressBook<N1>,
    ledger: &JobLedger,
    udp: &UdpSocket<N2>,
) {
    // Append to the ledger or send our new job request...
    // Communicate the new job across the network
    // Send the msg N times just in case of drop outs.
    // Can I zero copy and re-use a pbuf?
    let address_book_is_empty = {
        let guard = address_book.lock().await;
        guard.is_empty()
    };

    // TODO(failover): if Ledger becomes wrapped as
    // `LedgerState { ledger, updated_at }` (src/kinds.rs), the field
    // accesses below need adjusting accordingly, but this site should NOT
    // bump `updated_at` - a job upload isn't evidence the owner is alive,
    // same reasoning as the NewJob branch in udp.rs's udp_receiver.
    let is_owner = {
        let mut guard = ledger.lock().await;
        if let Some(state) = guard.as_mut()
            && let Some(addr) = my_ipaddr()
            && addr == state.ledger.owner
        {
            log_info!("I own the ledger. Adding the file");
            let _ = state.ledger.jobs.insert(guid);
            true
        } else {
            false
        }
    };

    if !is_owner && !address_book_is_empty {
        Message::send_new_job_alert(guid, udp).await;
    }
}

/// Sends the given job's file to every other known machine on the network.
async fn distribute_file<const N1: usize>(guid: Uuid, address_book: &AddressBook<N1>) {
    // Do not want to hold onto the lock
    let addrs = address_book.lock().await.clone();
    for (addr, _v) in addrs {
        log_info!("Sending file to {addr}");
        if let Err(err) = put_file(guid, addr, TCP_PORT).await {
            log_error!("Put Error: {err}");
        };
    }
}
