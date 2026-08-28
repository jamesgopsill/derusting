use alloc::vec::Vec;
use embassy_time::{Duration, Timer, WithTimeout as _};

use crate::{
    http::{BAD_REQUEST, INDEX_HTML, METHOD_NOT_ALLOWED, Method, OK, REQUEST_TIMEOUT},
    log_error, log_info,
    lwip::tcp_socket::TcpSocket,
};

#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat() {
    // NOTE. This can be useful to keep making progressing
    // if interrupts are missed by __pender. It's also
    // for our address book and sending our alive message
    // through UDP.
    loop {
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_task(sock: &'static TcpSocket) {
    loop {
        let html = detect_header(sock).await;

        let (start_line, headers, body) = split_request(&html).unwrap();
        let mut tokens = start_line.split(|&b| b == b' ');
        let Some(method) = tokens.next() else {
            sock.write_and_close(BAD_REQUEST.as_bytes());
            continue;
        };
        let method = match method {
            b"GET" => Method::Get,
            b"PUT" => Method::Put,
            _ => {
                sock.write_and_close(METHOD_NOT_ALLOWED.as_bytes());
                continue;
            }
        };

        let Some(url) = tokens.next() else {
            sock.write_and_close(BAD_REQUEST.as_bytes());
            continue;
        };
        let Ok(url) = str::from_utf8(url) else {
            sock.write_and_close(BAD_REQUEST.as_bytes());
            continue;
        };
        if url != "/" {
            sock.write_and_close(BAD_REQUEST.as_bytes());
            continue;
        }

        let Ok(headers) = str::from_utf8(headers) else {
            sock.write_and_close(BAD_REQUEST.as_bytes());
            continue;
        };

        match (method, url) {
            (Method::Get, "/") => {
                log_info!("/ GET request");
                sock.write_and_close(INDEX_HTML.as_bytes());
            }
            (Method::Put, "/") => {
                log_info!("/ PUT request");
                let mut content_length: usize = 0;
                let mut content_type: bool = false;
                for line in headers.lines() {
                    let Some((key, val)) = line.split_once(":") else {
                        sock.write_and_close(BAD_REQUEST.as_bytes());
                        continue;
                    };
                    match key.to_lowercase().as_str() {
                        "content-length" => {
                            let Ok(val) = val.trim().parse::<usize>() else {
                                log_error!("Could not parse content-length");
                                sock.write_and_close(BAD_REQUEST.as_bytes());
                                continue;
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
                        _ => {}
                    }
                }

                if !content_type {
                    sock.write_and_close(BAD_REQUEST.as_bytes());
                    continue;
                }

                if content_length == 0 {
                    sock.write_and_close(BAD_REQUEST.as_bytes());
                    continue;
                }

                if content_length > 1_000_000 {
                    sock.write_and_close(BAD_REQUEST.as_bytes());
                    return;
                }

                content_length = content_length.saturating_sub(body.len());

                let mut i = 0;
                let mut success = true;
                while content_length != 0 {
                    i += 1;
                    if i % 10 == 0 {
                        log_info!("{i} CL: {}", content_length);
                    }
                    match sock
                        .packets
                        .receive()
                        .with_timeout(Duration::from_millis(1_000))
                        .await
                    {
                        Ok(Some(packet)) => {
                            let bytes = packet.as_bytes();
                            let to_write = core::cmp::min(content_length, bytes.len());
                            // TODO: write to file
                            content_length = content_length.saturating_sub(to_write);
                        }
                        Ok(None) => {
                            log_info!("Connection Reset");
                            success = false;
                            // Reset by someone else
                            break;
                        }
                        Err(_) => {
                            log_info!("Timeout");
                            sock.write_and_close(REQUEST_TIMEOUT.as_bytes());
                            success = false;
                            break;
                        }
                    };
                }
                log_info!("Finished: {success}");
                if success {
                    sock.write_and_close(OK.as_bytes());
                }
            }
            (_, _) => {
                log_info!("Unsupported Route");
                sock.write_and_close(BAD_REQUEST.as_bytes());
            }
        }
    }
}

fn split_request(buf: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let delim = b"\r\n";
    let idx = buf.windows(delim.len()).position(|win| win == delim)?;
    let (start_line, rest) = buf.split_at(idx);
    let rest = &rest[2..];
    let delim = b"\r\n\r\n";
    let idx = rest.windows(delim.len()).position(|win| win == delim)?;
    let (headers, rest) = rest.split_at(idx);
    let body = &rest[4..];
    Some((start_line, headers, body))
}

async fn detect_header(sock: &TcpSocket) -> Vec<u8> {
    let mut html: Vec<u8> = Vec::new();
    loop {
        match sock.packets.receive().await {
            Some(packet) => {
                html.extend_from_slice(packet.as_bytes());
                if split_request(&html).is_some() {
                    break;
                };
            }
            None => {
                // Reset Detected
                html.clear();
            }
        }
    }
    html
}
