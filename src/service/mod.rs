use core::{
    marker::PhantomData,
    net::{Ipv4Addr, SocketAddrV4},
};

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::{Duration, Instant, Ticker, Timer};
use heapless::LinearMap;
use rand::{RngExt as _, rngs::SmallRng};
use static_cell::ConstStaticCell;
use uuid::Uuid;

use message::OwnedLedger;

use crate::service::{
    message::{MessageManager, OwnedGcode},
    transfer::FileTransfer,
};

mod message;
mod transfer;

const UDP_PORT: u16 = 9090;
const TCP_PORT: u16 = 8080;
const UDP_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, UDP_PORT);
const BROADCAST_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::BROADCAST, UDP_PORT);
const TCP_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, TCP_PORT);

/// Map of known peers to when we last heard from them (at most 32).
type PeerMap = LinearMap<SocketAddrV4, Instant, 32>;

/// The printer-specific services the derusting service depends on, kept
/// separate so the service itself stays platform agnostic.
pub trait Platform {
    /// The machine's own IPv4 address, if it has one.
    fn local(&self) -> Option<Ipv4Addr>;
    /// Whether the machine can start a new job right now (ready and idle).
    fn is_available(&self) -> bool;
    /// Starts manufacturing the job `id`; returns whether it was started.
    fn manufacture(&self, id: Uuid) -> bool;
    /// Writes a message to the platform's log.
    fn log(&self, args: core::fmt::Arguments);
}

/// A UDP socket used for peer-to-peer messaging.
pub trait UdpSocket
where
    Self: Sized,
{
    type Error: core::error::Error;

    /// Creates a new, unbound socket.
    fn new() -> Result<Self, Self::Error>;

    /// Binds to `socket` and returns the address actually bound (which
    /// differs from `socket` when port 0 is requested) along with the socket.
    fn bind(
        self,
        socket: SocketAddrV4,
    ) -> impl Future<Output = Result<(SocketAddrV4, Self), Self::Error>>;

    /// Waits for a datagram, returning the sender and the filled part of `buf`.
    fn receive<'a>(
        &self,
        buf: &'a mut [u8],
    ) -> impl Future<Output = Result<(SocketAddrV4, &'a [u8]), Self::Error>>;

    /// Sends `buf` to `remote`, returning the number of bytes sent.
    fn send(
        &self,
        remote: SocketAddrV4,
        buf: &[u8],
    ) -> impl Future<Output = Result<usize, Self::Error>>;

    /// Calls `send` repeatedly until all of `buf` has been sent.
    #[allow(unused)]
    fn send_all(
        &self,
        remote: SocketAddrV4,
        mut buf: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        async move {
            while !buf.is_empty() {
                let n = self.send(remote, buf).await?;
                buf = &buf[n..];
            }
            Ok(())
        }
    }
}

/// A TCP connection to a peer or an HTTP client.
pub trait TcpStream
where
    Self: Sized,
{
    type Error: core::error::Error;
    /// Opens a connection to `remote`.
    #[allow(unused)]
    fn connect(remote: SocketAddrV4) -> impl Future<Output = Result<Self, Self::Error>>;
    /// Reads available bytes into `buf` and returns the filled part. An empty
    /// result means the peer closed the connection.
    fn read<'a>(&self, buf: &'a mut [u8]) -> impl Future<Output = Result<&'a [u8], Self::Error>>;
    /// Writes some of `buf`, returning the number of bytes accepted (which may
    /// be less than `buf.len()`).
    fn write(&self, buf: &[u8]) -> impl Future<Output = Result<usize, Self::Error>>;
    /// Signals that we have finished sending (closes our write side).
    fn finish(&self) -> impl Future<Output = Result<(), Self::Error>>;

    /// Calls `write` repeatedly until all of `buf` has been written.
    fn write_all(&self, mut buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
        async move {
            while !buf.is_empty() {
                let n = self.write(buf).await?;
                buf = &buf[n..];
            }
            Ok(())
        }
    }

    /// Replies with the `200 OK` "pong" response and finishes the stream.
    fn pong(&self) -> impl Future<Output = Result<(), Self::Error>> {
        async {
            self.write_all(PONG.as_bytes()).await?;
            self.finish().await
        }
    }

    /// Replies with a `400 Bad Request` response and finishes the stream.
    fn bad_request(&self) -> impl Future<Output = Result<(), Self::Error>> {
        async {
            self.write_all(BAD_REQUEST.as_bytes()).await?;
            self.finish().await
        }
    }

    /// Replies with a `500 Internal Server Error` response and finishes the
    /// stream.
    fn internal_server_error(&self) -> impl Future<Output = Result<(), Self::Error>> {
        async {
            self.write_all(INTERNAL_SERVER_ERROR.as_bytes()).await?;
            self.finish().await
        }
    }
}

/// A TCP listener that accepts incoming connections.
pub trait TcpListener
where
    Self: Sized,
{
    type TcpStream: TcpStream;
    type Error: core::error::Error;
    /// Creates a new, unbound listener.
    fn new() -> Result<Self, Self::Error>;
    /// Binds to `socket`, starts listening and returns the address actually
    /// bound along with the listener.
    fn bind(
        self,
        socket: SocketAddrV4,
    ) -> impl Future<Output = Result<(SocketAddrV4, Self), Self::Error>>;
    /// Waits for the next incoming connection.
    fn accept(&self) -> impl Future<Output = Result<Self::TcpStream, Self::Error>>;
}

/// How a file is opened by `Vfs::open`.
pub enum VfsFlag {
    Read,
    Write,
}

/// A minimal virtual file system (the USB stick on the printer). Reading and
/// writing come from the `embedded_io` supertraits.
pub trait Vfs
where
    Self: Sized + embedded_io::Write + embedded_io::Read + embedded_io::ErrorType,
{
    /// Opens the file at `path` for reading or writing, per `flag`.
    fn open(path: &str, flag: VfsFlag) -> impl Future<Output = Result<Self, Self::Error>>;
    /// Deletes the file at `path`.
    fn delete(path: &str) -> impl Future<Output = Result<(), Self::Error>>;
    /// Renames the file `src` to `dest`.
    fn rename(src: &str, dest: &str) -> impl Future<Output = Result<(), Self::Error>>;
    /// Closes the file by consuming it (dropping it releases the handle).
    fn close(self) {}
    /// Whether `path` can be opened for reading.
    fn exists(path: &str) -> impl Future<Output = bool> {
        async { Self::open(path, VfsFlag::Read).await.is_ok() }
    }
}

/// Errors from creating a `Service`, from either the UDP or TCP side.
pub enum ServiceError<U, T>
where
    U: UdpSocket,
    T: TcpListener,
{
    Udp(U::Error),
    Tcp(T::Error),
}

/// The derusting service: shares a ledger of jobs between machines over UDP
/// (a token ring), accepts gcode uploads over TCP and starts jobs on the
/// local platform when it owns the ledger and is available.
pub struct Service<U, T, P, V>
where
    U: UdpSocket,
    T: TcpListener,
    P: Platform,
    V: Vfs,
{
    messenger: MessageManager<U>,
    tcp: T,
    platform: P,
    ledger: Mutex<NoopRawMutex, OwnedLedger>,
    peers: Mutex<NoopRawMutex, PeerMap>,
    vfs: PhantomData<V>,
    channel: Channel<NoopRawMutex, Uuid, 8>,
}

impl<U, T, P, V> Service<U, T, P, V>
where
    U: UdpSocket,
    T: TcpListener,
    P: Platform,
    V: Vfs,
{
    /// Creates the UDP and TCP sockets, binds them to `UDP_ADDR` and
    /// `TCP_ADDR`, and returns a service with an empty ledger and no peers.
    pub async fn new(platform: P) -> Result<Self, ServiceError<U, T>> {
        let udp = match U::new() {
            Ok(udp) => udp,
            Err(err) => return Err(ServiceError::Udp(err)),
        };
        let tcp = match T::new() {
            Ok(tcp) => tcp,
            Err(err) => return Err(ServiceError::Tcp(err)),
        };
        let udp = match udp.bind(UDP_ADDR).await {
            Ok((_, udp)) => udp,
            Err(err) => return Err(ServiceError::Udp(err)),
        };
        let tcp = match tcp.bind(TCP_ADDR).await {
            Ok((_, tcp)) => tcp,
            Err(err) => return Err(ServiceError::Tcp(err)),
        };
        let peers: Mutex<NoopRawMutex, PeerMap> = Mutex::new(LinearMap::new());
        let ledger: Mutex<NoopRawMutex, OwnedLedger> =
            Mutex::new(OwnedLedger::new(Ipv4Addr::UNSPECIFIED));
        let channel: Channel<NoopRawMutex, Uuid, 8> = Channel::new();
        Ok(Self {
            messenger: MessageManager::new(udp),
            tcp,
            platform,
            ledger,
            peers,
            vfs: PhantomData,
            channel,
        })
    }

    /// Runs all of the service's tasks (heartbeat, UDP receive, ledger, TCP
    /// accept, peer expiry and gcode broadcast) concurrently. These loop
    /// forever, so this only returns if the tasks are dropped.
    pub async fn run(&mut self) {
        let args = format_args!("Running derusting on {:?}", self.platform.local());
        self.platform.log(args);
        let fut_01 = self.heartbeat();
        let fut_02 = self.udp_receive_handler();
        let fut_03 = self.manage_ledger();
        let fut_04 = self.tcp_accept();
        let fut_05 = self.manage_peers();
        let fut_06 = self.broadcast_gcode();
        let fut = embassy_futures::join::join5(fut_01, fut_02, fut_03, fut_04, fut_05);
        let fut = embassy_futures::join::join(fut, fut_06);
        let _ = fut.await;
    }

    /// Broadcasts a heartbeat over UDP every 2 seconds so peers know we are
    /// alive.
    async fn heartbeat(&self) -> ! {
        let mut ticker = Ticker::every(Duration::from_secs(2));
        loop {
            ticker.next().await;
            match self.messenger.send_heartbeat().await {
                Ok(_) => self.platform.log(format_args!("HB Sent")),
                Err(_) => self.platform.log(format_args!("HB Error")),
            }
        }
    }

    /// Receives UDP messages forever: records the sender in the address book
    /// and handles each payload (new jobs, shared ledgers and gcode chunks of
    /// a file transfer; heartbeats and logs need no further action).
    async fn udp_receive_handler(&self) -> ! {
        static BUF: ConstStaticCell<[u8; 1024]> = ConstStaticCell::new([0u8; 1024]);
        let buf = BUF.take();
        let mut transfer: Option<FileTransfer<V>> = None;
        loop {
            let Some((remote, msg)) = self.messenger.receive(buf.as_mut()).await else {
                self.platform.log(format_args!("Packet Receive Error"));
                continue;
            };

            // Update address book.
            {
                let mut peers = self.peers.lock().await;
                // Max out at 32.
                let _ = peers.insert(remote, Instant::now());
            }

            match msg.payload {
                message::Payload::Heartbeat(_) => {
                    // No need to do anything as we have updated
                    // the address book as we do for all other
                    // messages.
                }
                message::Payload::NewJob(job) => {
                    let mut ledger = self.ledger.lock().await;
                    // can max out
                    let _ = ledger.insert(job);
                }
                message::Payload::Ledger(shared_ledger) => {
                    let mut ledger = self.ledger.lock().await;
                    *ledger = shared_ledger.into();
                }
                message::Payload::Log(_) => {
                    // Do nothing. This is for a logging
                    // tool for demo purposes
                }
                message::Payload::Gcode(gcode) => {
                    if gcode.chunk_id == 0
                        && transfer.is_none()
                        && let Ok(t) = FileTransfer::<V>::new(&gcode).await
                    {
                        let fargs =
                            format_args!("Transfer Recv Start: {:?}", self.platform.local());
                        let _ = self.messenger.send_log(fargs).await;
                        transfer = Some(t);
                        continue;
                    }
                    if let Some(t1) = transfer.take()
                        && let Ok(t2) = t1.digest(&gcode).await
                    {
                        transfer = t2;
                        if transfer.is_none() {
                            let fargs = format_args!(
                                "Transfer Recv Finished: {}",
                                self.platform.local().unwrap()
                            );
                            let _ = self.messenger.send_log(fargs).await;
                        }
                    }
                }
            }
        }
    }

    /// Every 10 seconds, drops peers we have not heard from in 20 seconds.
    pub async fn manage_peers(&self) -> ! {
        loop {
            Timer::after_secs(10).await;
            let mut peers = self.peers.lock().await;
            peers.retain(|_k, instant| instant.elapsed().as_secs() < 20);
        }
    }

    /// Drives the token-ring ledger every 5 seconds (after an initial 15
    /// second wait): takes ownership if the ledger has gone quiet, and when we
    /// own it starts a job if the platform is available, then passes the ledger
    /// to a random peer (or keeps it if we are alone).
    async fn manage_ledger(&self) -> ! {
        // Before we start. Lets give any other machines
        // on the network a chance to send us any ledger
        // in circulation.
        Timer::after_secs(15).await;
        let mut ticker = Ticker::every(Duration::from_secs(5));
        loop {
            ticker.next().await;

            // Only run if we have an ip address
            let Some(local) = self.platform.local() else {
                continue;
            };
            if local.is_unspecified() {
                continue;
            }

            // Get the ledger
            let mut ledger = self.ledger.lock().await;
            let fargs = format_args!("(manage_ledger) Ledger has {} job(s).", ledger.jobs().len());
            self.platform.log(fargs);

            // If I don't own the ledger then all I will do
            // is check whether I have not seen it change
            // ownership in a while.
            if ledger.owner != local {
                // It is not me. Lets check if it is an empty ip address.
                if ledger.updated.elapsed() > Duration::from_secs(20)
                    || ledger.owner.is_unspecified()
                {
                    let fargs = format_args!("Taking ownership.");
                    self.platform.log(fargs);
                    // It is not me but I haven't seen a more recent one being passed about.
                    // I will take it upon myself to start the process again.
                    ledger.owner = local;
                    let share = ledger.share();
                    let _ = self.messenger.send_share(share).await;
                }
                continue;
            }

            // I own the ledger
            if self.platform.is_available() {
                let guid = ledger.pick_one::<V>().await;
                let msg = format_args!("pick_one {guid:?}");
                self.platform.log(msg);

                if let Some(guid) = guid
                    && self.platform.manufacture(guid)
                {
                    ledger.remove(&guid);
                }
            }

            // Check whether I should send the ledger on.
            let peers = self.peers.lock().await;
            if peers.is_empty() {
                // I am the only one here so keep the ledger.
                self.platform
                    .log(format_args!("It's only me. Keeping ledger."));
                ledger.updated = Instant::now();
                // And I might as well tell everyone.
                let share = ledger.share();
                let _ = self.messenger.send_share(share).await;
                continue;
            }

            // Send it to another machine
            let mut rng: SmallRng = rand::make_rng();
            let idx = rng.random_range(0..peers.len());
            match peers.iter().nth(idx) {
                Some(peer) => {
                    ledger.owner = *peer.0.ip();
                    let share = ledger.share();
                    let _ = self.messenger.send_share(share).await;
                }
                None => {
                    ledger.updated = Instant::now();
                    // Ok, I will keep it
                    let share = ledger.share();
                    let _ = self.messenger.send_share(share).await;
                    continue;
                }
            }
        }
    }

    /// Serves HTTP over TCP, one connection at a time. `GET /` returns the
    /// web page; `PUT /` with a gcode body saves the upload to the USB stick
    /// (as `.partial`, renamed to `.gcode` once complete), announces the new job
    /// to peers, adds it to the ledger and queues it for UDP broadcast.
    async fn tcp_accept(&self) -> ! {
        let mut buf = [0u8; 1024]; // Keep it out of the stack frame
        loop {
            // Accept a new connection
            let Ok(stream) = self.tcp.accept().await else {
                self.platform.log(format_args!("TCP accept err."));
                continue;
            };

            // Get the first batch of data
            let Ok(data) = stream.read(&mut buf).await else {
                self.platform.log(format_args!("TCP stream read err"));
                let _ = stream.internal_server_error().await;
                continue;
            };

            // Does it contain the necessary headers
            let Some((start_line, headers, body)) = split_request(data) else {
                if stream.bad_request().await.is_err() {
                    self.platform.log(format_args!("TCP stream write err"));
                };
                continue;
            };

            // Check whether we handle the start_line
            let method = match check_start_line(start_line) {
                Ok(method) => method,
                Err(e) => {
                    if stream.write(e.as_bytes()).await.is_err() {
                        self.platform.log(format_args!("TCP stream write err"));
                    };
                    continue;
                }
            };

            // Check the method.
            match method {
                Method::Get => {
                    self.platform.log(format_args!("/ GET"));
                    if stream.write(INDEX_HTML.as_bytes()).await.is_err() {
                        self.platform.log(format_args!("TCP stream write err"));
                    };
                    if stream.finish().await.is_err() {
                        self.platform.log(format_args!("TCP stream finish err"));
                        continue;
                    }
                    continue;
                }
                Method::Put => {
                    self.platform.log(format_args!("/ PUT"));

                    let info = check_put_header(headers);
                    if !info.is_gcode
                        || info.size.is_none()
                        || info.size.is_some_and(|s| s == 0 || s > 1_000_000)
                    {
                        if stream.bad_request().await.is_err() {
                            self.platform.log(format_args!("TCP stream write err"));
                        };
                        continue;
                    }

                    let guid = match info.guid {
                        Some(guid) => {
                            self.platform
                                .log(format_args!("Receiving file from machine"));
                            guid
                        }
                        None => Uuid::new_v4(),
                    };
                    let partial_path = heapless::format!(64; "/usb/{}.partial", guid).unwrap();
                    let Ok(mut fil) = V::open(partial_path.as_str(), VfsFlag::Write).await else {
                        self.platform.log(format_args!("File open error"));
                        let _ = stream.internal_server_error().await;
                        continue;
                    };

                    let mut content_length = info.size.unwrap();

                    // Write bytes to file from current chunk
                    let to_write = core::cmp::min(content_length, body.len());
                    let _ = fil.write(&body[..to_write]);
                    content_length = content_length.saturating_sub(to_write);

                    // Continue reading bytes.
                    let mut more_data_needed = true;
                    let mut failed = false;
                    loop {
                        let Ok(data) = stream.read(&mut buf).await else {
                            self.platform.log(format_args!("TCP stream read err"));
                            failed = true;
                            break;
                        };
                        if data.is_empty() {
                            break;
                        }
                        let to_write = core::cmp::min(content_length, data.len());
                        let _ = fil.write(&data[..to_write]);
                        content_length = content_length.saturating_sub(to_write);
                        if content_length == 0 {
                            more_data_needed = false;
                            break;
                        }
                    }

                    // If the tcp failed during reading.
                    if failed {
                        let _ = fil.flush();
                        fil.close();
                        let _ = V::delete(&partial_path).await;
                        if stream.internal_server_error().await.is_err() {
                            self.platform.log(format_args!("TCP stream write err"));
                        };
                        continue;
                    }

                    // The stream did not give us enough data
                    if more_data_needed {
                        let _ = fil.flush();
                        fil.close();
                        let _ = V::delete(&partial_path).await;
                        if stream.bad_request().await.is_err() {
                            self.platform.log(format_args!("TCP stream write err"));
                        };
                        continue;
                    }

                    // Received all the data. Lets close and
                    // rename it.
                    let _ = fil.flush();
                    fil.close();

                    let final_path = heapless::format!(64; "/usb/{}.gcode", guid).unwrap();
                    if V::rename(partial_path.as_str(), final_path.as_str())
                        .await
                        .is_err()
                    {
                        if stream.internal_server_error().await.is_err() {
                            self.platform.log(format_args!("TCP stream write err"));
                        };
                        continue;
                    };

                    let _ = self.messenger.send_new_job(guid).await;

                    // Add it to our ledger. Does it matter if we own
                    // it or not so we stay up to date.
                    let mut ledger = self.ledger.lock().await;
                    let _ = ledger.insert(guid);

                    if stream.pong().await.is_err() {
                        self.platform.log(format_args!("TCP stream write err"));
                    };

                    // Could end up blocking if too many
                    // files are uploaded too quickly.
                    self.channel.send(guid).await;

                    continue;
                }
            }
        }
    }

    /// Waits for uploaded files to be queued on the channel, then broadcasts
    /// each one over UDP in 768-byte chunks, 500 ms apart.
    async fn broadcast_gcode(&self) -> ! {
        loop {
            let guid = self.channel.receive().await;
            let path = heapless::format!(64; "/usb/{}.gcode", guid).unwrap();

            // Tcp complete now share the file around UDP but
            // note this should be moved out of this separate task
            // as it currently prevents new tcp streams.
            let Ok(mut fil) = V::open(&path, VfsFlag::Read).await else {
                continue;
            };
            let mut chunk = OwnedGcode {
                guid,
                chunk_id: 0,
                last_chunk: false,
                data: [0u8; 768],
                len: 768,
            };
            loop {
                let Ok(res) = fil.read(&mut chunk.data) else {
                    self.platform.log(format_args!("Read error"));
                    break;
                };
                if res < chunk.data.len() {
                    // EOF
                    chunk.last_chunk = true;
                    chunk.len = res;
                    let _ = self.messenger.send_gcode(chunk.share()).await;
                    break;
                }
                let _ = self.messenger.send_gcode(chunk.share()).await;
                chunk.chunk_id += 1;
                Timer::after_millis(500).await
            }
        }
    }
}

/// The HTTP methods the TCP server handles.
enum Method {
    Get,
    Put,
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

const PONG: &str = "HTTP/1.1 200 OK\r\nContent-length:4\r\nConnection: close\r\n\r\npong";
const BAD_REQUEST: &str =
    "HTTP/1.1 400 Bad Request\r\nContent-length:0\r\nConnection: close\r\n\r\n";
const METHOD_NOT_ALLOWED: &str =
    "HTTP/1.1 405 Method Not Allowed\r\nContent-length:0\r\nConnection: close\r\n\r\n";
const INTERNAL_SERVER_ERROR: &str =
    "HTTP/1.1 500 Internal Server Error\r\nContent-length:0\r\nConnection: close\r\n\r\n";
const HTML_STR: &str = include_str!("../../assets/index.html");
const INDEX_HTML: &str = const_format::formatcp!(
    "HTTP/1.1 200 OK\r\n\
    Content-Type: text/html\r\n\
    Content-Length: {}\r\n\
    Connection: close\r\n\r\n\
    {}",
    HTML_STR.len(),
    HTML_STR
);
