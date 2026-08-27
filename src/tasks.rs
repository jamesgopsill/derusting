use core::array;

use alloc::{boxed::Box, vec::Vec};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer, WithTimeout as _};

use crate::{
    MAX_HANDLERS, TcpChannels,
    http::{BAD_REQUEST, INDEX_HTML, INTERNAL_SERVER_ERROR, METHOD_NOT_ALLOWED, Method, OK},
    log_error, log_info,
    lwip::handler::Handler,
};

#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat() {
    // NOTE. This can be useful to keep making progressing
    // if interrupts are missed by __pender. It's also
    // for our address book and sending our alive message
    // through UDP.
    loop {
        Timer::after_millis(100).await;
    }
}

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_pool(spawner: Spawner, tcp_channels: &'static TcpChannels) {
    log_info!("TCP Pool Task Started");
    loop {
        log_info!("Waiting for channel");
        let h = tcp_channels.receive().await;
        log_info!("Channel Received");
        match tcp_channel_handler(h) {
            Ok(t) => {
                log_info!("Spawning task");
                spawner.spawn(t);
            }
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

#[embassy_executor::task(pool_size = MAX_HANDLERS)]
pub async fn tcp_channel_handler(h: Box<Handler>) {
    let mut html: Vec<u8> = Vec::new();
    // TODO: add a timeout.
    loop {
        let (len, array) = h.channel.receive().await;
        html.extend_from_slice(&array[..len]);
        if split_request(&html).is_some() {
            break;
        };
    }

    let (start_line, headers, body) = split_request(&html).unwrap();

    let mut tokens = start_line.split(|&b| b == b' ');
    let Some(method) = tokens.next() else {
        h.write_and_close(BAD_REQUEST.as_bytes());
        return;
    };
    let method = match method {
        b"GET" => Method::Get,
        b"PUT" => Method::Put,
        _ => {
            h.write_and_close(METHOD_NOT_ALLOWED.as_bytes());
            return;
        }
    };

    let Some(url) = tokens.next() else {
        h.write_and_close(BAD_REQUEST.as_bytes());
        return;
    };
    let Ok(url) = str::from_utf8(url) else {
        h.write_and_close(BAD_REQUEST.as_bytes());
        return;
    };
    if url != "/" {
        h.write_and_close(BAD_REQUEST.as_bytes());
        return;
    }

    let Ok(headers) = str::from_utf8(headers) else {
        h.write_and_close(BAD_REQUEST.as_bytes());
        return;
    };

    match (method, url) {
        (Method::Get, "/") => {
            h.write_and_close(INDEX_HTML.as_bytes());
        }
        (Method::Put, "/") => {
            let mut content_length: usize = 0;
            let mut content_type: bool = false;
            for line in headers.lines() {
                let Some((key, val)) = line.split_once(":") else {
                    h.write_and_close(BAD_REQUEST.as_bytes());
                    return;
                };
                match key.to_lowercase().as_str() {
                    "content-length" => {
                        let Ok(val) = val.trim().parse::<usize>() else {
                            log_error!("Could not parse content-length");
                            h.write_and_close(BAD_REQUEST.as_bytes());
                            return;
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
                h.write_and_close(BAD_REQUEST.as_bytes());
                return;
            }

            if content_length == 0 {
                h.write_and_close(BAD_REQUEST.as_bytes());
                return;
            }

            if content_length > 1_000_000 {
                h.write_and_close(BAD_REQUEST.as_bytes());
                return;
            }

            content_length = content_length.saturating_sub(body.len());

            while content_length != 0 {
                match h
                    .channel
                    .receive()
                    .with_timeout(Duration::from_millis(500))
                    .await
                {
                    Ok((len, _array)) => {
                        let to_write = core::cmp::min(content_length, len);
                        content_length = content_length.saturating_sub(to_write);
                    }
                    Err(_) => {
                        h.write_and_close(INTERNAL_SERVER_ERROR.as_bytes());
                        return;
                    }
                };
            }
            log_info!("Finished");

            h.write_and_close(OK.as_bytes());
        }
        (_, _) => h.write_and_close(BAD_REQUEST.as_bytes()),
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
