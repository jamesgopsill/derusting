use core::pin::Pin;

use embassy_time::Timer;
use embedded_io::{Read as _, Write as _};

use crate::{
    fs::{self, ReadBytes, WriteBytes},
    http::{BAD_REQUEST, INDEX_HTML, INTERNAL_SERVER_ERROR, METHOD_NOT_ALLOWED, Method, OK},
    log_error, log_info,
    lwip::{
        packet_buffer::PacketBuffer,
        tcp::{TcpConnection, TcpListener},
        udp::UdpSocket,
    },
    tasks::{
        messages::{Chunk, NetworkMessage},
        rng::generate_uuid_v7,
        udp::{ADDRESS_BOOK, LEDGER, make_path},
    },
};

/// This task receives new tcp handlers and spawns
/// tasks to manage each one.
pub async fn tcp_worker<const N: usize, const M: usize, const O: usize>(
    tcp: Pin<&TcpListener<N, M>>,
    udp: Pin<&UdpSocket<O>>,
) {
    loop {
        tcp.as_ref()
            .with_connection(async |conn| handle_conn(conn, udp.as_ref()).await)
            .await;
    }
}

/// A task that handles TCP requests for the printer. There is only `GET /` and `PUT /` to
/// retrieve the submission and put files onto the network for processing.
pub async fn handle_conn<const N: usize, const M: usize>(
    conn: Pin<&mut TcpConnection<N>>,
    udp: Pin<&UdpSocket<M>>,
) {
    log_info!("New Task");
    let Some(pbuf) = conn.as_ref().receive().await else {
        log_error!("Handle Closed");
        return;
    };

    let mut iter = pbuf.into_iter();

    let Some(chunk) = iter.next() else {
        let _ = conn.response(BAD_REQUEST.as_bytes());
        return;
    };

    let Some((start_line, headers, body)) = split_request(chunk) else {
        let _ = conn.response(BAD_REQUEST.as_bytes());
        return;
    };

    let method = match check_start_line(start_line) {
        Ok(method) => method,
        Err(e) => {
            let _ = conn.response(e.as_bytes());
            return;
        }
    };

    match method {
        Method::Get => {
            log_info!("/ GET request");
            let _ = conn.response(INDEX_HTML.as_bytes());
        }
        Method::Put => {
            log_info!("/ PUT request");
            let Ok(mut content_length) = check_put_header(headers) else {
                let _ = conn.response(BAD_REQUEST.as_bytes());
                return;
            };

            let guid = generate_uuid_v7();
            let path = make_path(&guid, true);

            let Ok(mut f) = fs::open(path.as_c_str(), WriteBytes) else {
                let _ = conn.response(INTERNAL_SERVER_ERROR.as_bytes());
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
            let _ = conn.response(OK.as_bytes());

            // Append to the ledger or send our new job request...
            // Communicate the new job across the network
            // Send the msg 5 times just in case of drop outs.
            // Can I zero copy and re-use a pbuf?
            if let Some(ledger) = LEDGER.lock().await.borrow_mut().as_mut() {
                log_info!("I own the ledger. Adding the file");
                let _ = ledger.jobs.insert(guid);
            } else {
                // OPTMISATION: If there are other machines to send to
                if !ADDRESS_BOOK.lock().await.borrow().is_empty() {
                    let msg = NetworkMessage::new_job(guid);
                    for _i in 0..5 {
                        if let Some(pbuf) = PacketBuffer::alloc(&msg) {
                            if udp.broadcast(pbuf, 9090).is_err() {
                                log_error!("Broadcasting job failed.");
                            } else {
                                log_info!("Job message sent");
                            }
                        }
                        Timer::after_millis(200).await;
                    }
                }
            }

            // Now open, read and send the file chunks to propogate
            // it through the network. Only if there are machines
            // to broadcast to.
            if !ADDRESS_BOOK.lock().await.borrow().is_empty() {
                let mut n: usize = 0;
                let mut len: usize = usize::MAX;
                if let Ok(mut f) = fs::open(&path, ReadBytes) {
                    n += 1;
                    let mut bytes = [0u8; 768];
                    while len != 0 {
                        log_info!("Sending: {n}");
                        if let Ok(l) = f.read(&mut bytes) {
                            len = l;
                            let chunk = Chunk {
                                guid,
                                chunk_id: n as u16,
                                last_chunk: len == 0,
                                len: len as u16,
                                chunk: bytes,
                            };
                            let msg = NetworkMessage::Chunk(chunk);
                            // Send a repeated set of messages
                            for _i in 0..3 {
                                if let Some(pbuf) = PacketBuffer::alloc(&msg)
                                    && udp.broadcast(pbuf, 9090).is_err()
                                {
                                    log_error!("Broadcasting job chunk failed.");
                                }
                                Timer::after_millis(200).await;
                            }
                        }
                    }
                }
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

/// Analyses the PUT header to ensure it features the information
/// we require to process the request.
fn check_put_header(headers: &str) -> Result<usize, &'static str> {
    let mut content_length: usize = 0;
    let mut content_type: bool = false;
    for line in headers.lines() {
        let Some((key, val)) = line.split_once(":") else {
            return Err(BAD_REQUEST);
        };
        match key {
            "content-length" => {
                let Ok(val) = val.trim().parse::<usize>() else {
                    log_error!("Could not parse content-length");
                    return Err(BAD_REQUEST);
                };
                content_length = val;
            }
            "Content-Length" => {
                let Ok(val) = val.trim().parse::<usize>() else {
                    return Err(BAD_REQUEST);
                };
                content_length = val;
            }
            "content-type" =>
            {
                #[allow(clippy::collapsible_match)]
                if val.trim() == "text/x.gcode" {
                    content_type = true;
                }
            }
            "Content-Type" =>
            {
                #[allow(clippy::collapsible_match)]
                if val.trim() == "text/x.gcode" {
                    content_type = true;
                }
            }
            _ => {}
        }
    }

    if !content_type {
        return Err(BAD_REQUEST);
    }

    if content_length == 0 {
        return Err(BAD_REQUEST);
    }

    if content_length > 1_000_000 {
        return Err(BAD_REQUEST);
    }

    Ok(content_length)
}
