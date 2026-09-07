use alloc::boxed::Box;
use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io::{Read as _, Write as _};

use crate::{
    fs::{File, ReadBytes, WriteBytes},
    http::*,
    log_error, log_info,
    lwip::{
        UDP_PORT,
        packet_buffer::PacketBuffer,
        tcp::{TcpHandle, TcpHandler},
        udp::UdpSocket,
    },
    tasks::{
        messages::{Chunk, NetworkMessage},
        rng::generate_uuid_v7,
        udp::{ADDRESS_BOOK, LEDGER, make_path},
    },
};

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_handler_task(
    handler: &'static TcpHandler,
    udp: &'static UdpSocket,
    spawner: Spawner,
) {
    log_info!("TCP handler task started");
    loop {
        let new_handle = handler.receive().await;
        log_info!("Received new handle");
        match tcp_handle_task(new_handle, udp) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
pub async fn tcp_handle_task(mut handle: Box<TcpHandle>, udp: &'static UdpSocket) {
    log_info!("New Task");
    let Some(pbuf) = handle.packets.receive().await else {
        log_error!("Handle Closed");
        handle.close();
        return;
    };

    let mut iter = pbuf.into_iter();

    let Some(chunk) = iter.next() else {
        handle.respond(BAD_REQUEST.as_bytes());
        return;
    };

    let Some((start_line, headers, body)) = split_request(chunk) else {
        handle.respond(BAD_REQUEST.as_bytes());
        return;
    };

    let method = match check_start_line(start_line) {
        Ok(method) => method,
        Err(e) => {
            handle.respond(e.as_bytes());
            return;
        }
    };

    match method {
        Method::Get => {
            log_info!("/ GET request");
            handle.respond(INDEX_HTML.as_bytes());
        }
        Method::Put => {
            log_info!("/ PUT request");
            let Ok(mut content_length) = check_put_header(headers) else {
                handle.respond(BAD_REQUEST.as_bytes());
                return;
            };

            let guid = generate_uuid_v7();
            let path = make_path(&guid);

            let Ok(mut f) = File::open(path.as_c_str(), WriteBytes) else {
                handle.respond(INTERNAL_SERVER_ERROR.as_bytes());
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
                    let Some(pbuf) = handle.packets.receive().await else {
                        log_error!("Handle Reset");
                        handle.close();
                        File::<ReadBytes>::delete(&path);
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
            handle.respond(OK.as_bytes());
            // dry_print();

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
                            if udp.broadcast(pbuf, UDP_PORT).is_err() {
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
                if let Ok(mut f) = File::open(&path, ReadBytes) {
                    n += 1;
                    let mut bytes = [0u8; 768];
                    while len != 0 {
                        log_info!("Sending: {n}");
                        if let Ok(l) = f.read(&mut bytes) {
                            len = l;
                            let msg = Chunk {
                                guid,
                                chunk_id: n as u16,
                                last_chunk: len == 0,
                                len: len as u16,
                                chunk: bytes,
                            };
                            // Send a repeated set of messages
                            for _i in 0..5 {
                                if let Some(pbuf) = PacketBuffer::alloc(&msg)
                                    && udp.broadcast(pbuf, UDP_PORT).is_err()
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
