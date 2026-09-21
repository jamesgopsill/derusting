use const_format::formatcp;

/// Fixed sized responses for common errors and ok HTTP messages.
pub const INTERNAL_SERVER_ERROR: &str =
    "HTTP/1.1 500 Internal Server Error\r\nContent-length:0\r\nConnection: close\r\n\r\n";
pub const OK: &str = "HTTP/1.1 200 OK\r\nContent-length:4\r\nConnection: close\r\n\r\npong";
pub const BAD_REQUEST: &str =
    "HTTP/1.1 400 Bad Request\r\nContent-length:0\r\nConnection: close\r\n\r\n";
#[allow(unused)]
pub const REQUEST_TIMEOUT: &str =
    "HTTP/1.1 408 Request Timeout\r\nContent-length:0\r\nConnection: close\r\n\r\n";
pub const METHOD_NOT_ALLOWED: &str =
    "HTTP/1.1 405 Method Not Allowed\r\nContent-length:0\r\nConnection: close\r\n\r\n";
#[allow(unused)]
pub const SERVICE_UNAVAILABLE: &str =
    "HTTP/1.1 503 Service Unavailable\r\nContent-length:0\r\nConnection: close\r\n\r\n";

/// Derusting custom upload form.
const HTML_STR: &str = include_str!("../assets/index.html");
pub const INDEX_HTML: &str = formatcp!(
    "HTTP/1.1 200 OK\r\n\
    Content-Type: text/html\r\n\
    Content-Length: {}\r\n\
    Connection: close\r\n\r\n\
    {}",
    HTML_STR.len(),
    HTML_STR
);

/// The HTTP methods this firmware's minimal server understands.
pub enum Method {
    Get,
    Put,
}
