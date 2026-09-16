use core::pin::Pin;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Timer;
use embedded_io::{Read as _, Write as _};
use uuid::Uuid;

use crate::{
    ADDRESS_BOOK_ENTRIES, MAX_TCP_CONNECTION_CHANNEL_SIZE, MAX_TCP_CONNECTIONS, UDP_CHANNEL_SIZE,
    UDP_PORT,
    fs::{self, ReadBytes, WriteBytes},
    http::{BAD_REQUEST, INDEX_HTML, INTERNAL_SERVER_ERROR, METHOD_NOT_ALLOWED, Method, OK},
    kinds::{AddressBook, JobLedger},
    log_error, log_info,
    lwip::{
        my_ipaddr,
        packet_buffer::PacketBuffer,
        tcp::{TcpConnection, TcpListener},
        udp::UdpSocket,
    },
    tasks::{
        messages::{Chunk, NetworkMessage},
        rng::generate_uuid_v7,
        udp::make_path,
    },
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
        tcp.with_connection(async |conn| handle_conn(conn, udp, address_book, ledger).await)
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
            log_info!("/ GET request");
            let _ = conn.response(INDEX_HTML.as_bytes()).await;
        }
        Method::Put => {
            log_info!("/ PUT request");
            let Ok(mut content_length) = check_put_header(headers) else {
                let _ = conn.response(BAD_REQUEST.as_bytes()).await;
                return;
            };

            let guid = generate_uuid_v7();
            let path = make_path(&guid, true);

            let Ok(mut f) = fs::open(path.as_c_str(), WriteBytes) else {
                let _ = conn.response(INTERNAL_SERVER_ERROR.as_bytes()).await;
                return;
            };

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
            let new_path = make_path(&guid, false);
            // Rename from partial to full.
            fs::rname(&path, &new_path);
            let _ = conn.response(OK.as_bytes()).await;

            append_to_ledger(guid, address_book, ledger, udp).await;

            broadcast_file(guid, address_book, udp).await;
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

/// Analyses the PUT header to ensure it features the information
/// we require to process the request.
fn check_put_header(headers: &str) -> Result<usize, &'static str> {
    let mut content_length = None;
    let mut content_type_ok = false;

    for line in headers.lines() {
        let Some((key, val)) = line.split_once(':') else {
            return Err(BAD_REQUEST);
        };
        let key = key.trim();
        let val = val.trim();

        if key.eq_ignore_ascii_case("content-length") {
            content_length = val.parse::<usize>().ok();
        } else if key.eq_ignore_ascii_case("content-type") && val == "text/x.gcode" {
            content_type_ok = true;
        }
    }

    let len = content_length.ok_or(BAD_REQUEST)?;
    if !content_type_ok || len == 0 || len > 1_000_000 {
        return Err(BAD_REQUEST);
    }

    Ok(len)
}

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

    let is_owner = {
        let mut guard = ledger.lock().await;
        if let Some(ledge) = guard.as_mut()
            && let Some(addr) = my_ipaddr()
            && addr == ledge.owner
        {
            log_info!("I own the ledger. Adding the file");
            let _ = ledge.jobs.insert(guid);
            // Send the ledger out. Everyone keeps a copy.
            for _ in 0..3 {
                {
                    let msg = NetworkMessage::Ledger(ledge.clone());
                    if let Some(pbuf) = PacketBuffer::alloc(&msg)
                        && udp.broadcast(pbuf, UDP_PORT).await.is_err()
                    {
                        log_error!("Broadcasting job failed.");
                    }
                }
                Timer::after_millis(100).await;
            }
            true
        } else {
            false
        }
    };

    if !is_owner && !address_book_is_empty {
        for _ in 0..3 {
            // Scope `msg` so it does not persist across `Timer::after_millis`
            {
                let msg = NetworkMessage::new_job(guid);
                if let Some(pbuf) = PacketBuffer::alloc(&msg)
                    && udp.broadcast(pbuf, UDP_PORT).await.is_err()
                {
                    log_error!("Broadcasting job failed.");
                }
            }
            Timer::after_millis(100).await;
        }
    }
}

static BROADCAST_BUF: Mutex<ThreadModeRawMutex, [u8; 768]> = Mutex::new([0u8; 768]);

async fn broadcast_file<const N1: usize, const N2: usize>(
    guid: Uuid,
    address_book: &AddressBook<N1>,
    udp: &UdpSocket<N2>,
) {
    // Now open, read and send the file chunks to propogate
    // it through the network. Only if there are machines
    // to broadcast to.
    let address_book_is_empty = {
        let guard = address_book.lock().await;
        guard.is_empty()
    };
    if address_book_is_empty {
        return;
    }

    let path = make_path(&guid, false);

    let Ok(mut f) = fs::open(&path, ReadBytes) else {
        log_error!("Failed to open file for broadcast");
        return;
    };

    let mut chunk_id: u16 = 0;
    let mut buf = BROADCAST_BUF.lock().await;

    loop {
        let bytes_read = match f.read(buf.as_mut_slice()) {
            Ok(n) => n,
            Err(e) => {
                log_error!("File read error: {e:?}");
                break;
            }
        };

        chunk_id += 1;
        let eof = bytes_read == 0;

        for _ in 0..3 {
            // Scope chunk/msg so they are destroyed before the 200 ms sleep
            {
                let chunk = Chunk {
                    guid,
                    chunk_id,
                    last_chunk: eof,
                    len: bytes_read as u16,
                    chunk: &buf[..bytes_read],
                };
                let msg = NetworkMessage::Chunk(chunk);

                if let Some(pbuf) = PacketBuffer::alloc(&msg)
                    && udp.broadcast(pbuf, UDP_PORT).await.is_err()
                {
                    log_error!("Broadcasting job chunk failed.");
                }
            }
            Timer::after_millis(100).await;
        }

        if eof {
            break;
        }
    }
}
