use alloc::boxed::Box;
use embassy_executor::Spawner;

use crate::{
    fs::{File, FileMode},
    http::*,
    log_error, log_info,
    lwip::tcp::{TcpHandle, TcpHandler},
};

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_handler_task(handler: &'static TcpHandler, spawner: Spawner) {
    log_info!("TCP handler task started");
    loop {
        let new_handle = handler.receive().await;
        log_info!("Received new handle");
        match tcp_handle_task(new_handle) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
pub async fn tcp_handle_task(mut handle: Box<TcpHandle>) {
    log_info!("New Task");
    let Some(pbuf) = handle.packets.receive().await else {
        log_error!("Handle Closed");
        handle.close();
        return;
    };

    let mut iter = pbuf.iter();

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

            let Ok(f) = File::open(c"/usb/rust.gcode", FileMode::Write) else {
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
                        return;
                    };
                    for chunk in pbuf.iter() {
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
