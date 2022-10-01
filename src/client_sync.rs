//! ```
//! use std::io::{Read, Write};
//! use tokio::io::{AsyncReadExt, AsyncWriteExt};
//! use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
//! use tokio::sync::mpsc;
//! use tinyroute::client::{ClientSender, ClientReceiver, ClientMessage, connect, Client, TcpClient};
//! use tinyroute::frame::Frame;
//!
//! #[tokio::main]
//! async fn main() {
//!     # let _ = async move {
//!     let client = TcpClient::connect("127.0.0.1:5000").await.unwrap();
//!     let heartbeat = std::time::Duration::from_secs(30);
//!     let (send, rec) = connect(client, Some(heartbeat));
//!     let (tx, rx) = mpsc::channel(10);
//!
//!     tokio::spawn(receiver(rec, tx));
//!     tokio::spawn(sender(send, rx));
//!     # };
//! }
//!
//! async fn receiver(mut rec: ClientReceiver, mut tx: mpsc::Sender<Vec<u8>>) {
//!     loop {
//!         let msg = rec.recv_async().await.unwrap();
//!         tx.send(msg);
//!     }
//! }
//!
//! async fn sender(mut send: ClientSender, mut rx: mpsc::Receiver<Vec<u8>>) {
//!     loop {
//!         let msg = rx.recv().await.unwrap();
//!         let framed_msg = Frame::frame_message(&msg);
//!         send.send(ClientMessage::Payload(framed_msg));
//!     }
//! }
//! ```
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::Duration;

use log::{error, info};

use crate::errors::{Error, Result};
use crate::frame::{Frame, FrameOutput, FramedMessage};
use crate::ADDRESS_SEP;
use crate::client::jitter;
use flume::{Receiver, Sender};

/// Type alias for `tokio::mpsc::Receiver<Vec<u8>>`
pub type ClientReceiver = Receiver<Vec<u8>>;
/// Type alias for `tokio::mpsc::Sender<ClientMessage>`
pub type ClientSender = Sender<ClientMessage>;

/// Client message: a message sent by a client.
/// The server will only ever see the payload bytes.
// TODO: shared with async version
#[derive(Debug)]
pub enum ClientMessage {
    /// Shut down the client
    Quit,
    /// Bunch of delicious bytes
    Payload(FramedMessage),
    Raw(Vec<u8>),
    /// Heartbeats
    Heartbeat,
}

impl ClientMessage {
    /// Create a `ClientMessage::Payload` from a channel and payload.
    ///
    /// ```
    /// use tinyroute::client::ClientMessage;
    ///
    /// let channel = b"chan";
    /// let payload = b"hello world";
    ///
    /// let client_message = ClientMessage::channel_payload(channel, payload);
    /// ```
    pub fn channel_payload(channel: &[u8], payload: &[u8]) -> Self {
        let mut buf = Vec::with_capacity(channel.len() + 1 + payload.len());
        buf.extend_from_slice(channel);
        buf.push(ADDRESS_SEP);
        buf.extend_from_slice(payload);
        let framed_message = Frame::frame_message(&buf);
        ClientMessage::Payload(framed_message)
    }

    pub fn channel_payload_raw(channel: &[u8], payload: &[u8]) -> Self {
        let mut buf = Vec::with_capacity(channel.len() + 1 + payload.len());
        buf.extend_from_slice(channel);
        buf.push(ADDRESS_SEP);
        buf.extend_from_slice(payload);
        ClientMessage::Raw(buf)
    }
}

/// A client connection
pub trait Client {
    /// The reading half of the connection
    type Reader: std::io::Read + Send + 'static;
    /// The writing half of the connection
    type Writer: std::io::Write + Send + 'static;

    /// Split the connection into a reader / writer pair
    fn split(self) -> Result<(Self::Reader, Self::Writer)>;
}

/// ```
/// # use tinyroute::client::UdsClient;
/// # async fn run() {
/// let uds_client = UdsClient::connect("/tmp/tinyroute.sock").await.unwrap();
/// # }
/// ```
pub struct UdsClient {
    inner: UnixStream,
}

impl UdsClient {
    /// Establish a tcp connection
    pub fn connect(addr: impl AsRef<Path>) -> Result<Self> {
        let inner = UnixStream::connect(addr)?;
        let inst = Self { inner };
        Ok(inst)
    }
}

impl Client for UdsClient {
    type Reader = UnixStream;
    type Writer = UnixStream;

    fn split(self) -> Result<(Self::Reader, Self::Writer)> {
        let reader = self.inner;
        let writer = reader.try_clone()?;
        Ok((reader, writer))
    }
}

/// ```
/// # use tinyroute::client::TcpClient;
/// # async fn run() {
/// let tcp_client = TcpClient::connect("127.0.0.1:5000").await.unwrap();
/// # }
/// ```
pub struct TcpClient {
    pub inner: TcpStream,
}

impl TcpClient {
    /// Establish a tcp connection
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let inner = TcpStream::connect(addr)?;
        let inst = Self { inner };
        Ok(inst)
    }
}

impl Client for TcpClient {
    type Reader = std::net::TcpStream;
    type Writer = std::net::TcpStream;

    fn split(self) -> Result<(Self::Reader, Self::Writer)> {
        let reader = self.inner;
        let writer = reader.try_clone()?;

        Ok((reader, writer))
    }
}

/// Get a [`ClientSender`] and [`ClientReceiver`] pair
pub fn connect(connection: impl Client, heartbeat: Option<Duration>) -> Result<(ClientSender, ClientReceiver)> {
    let (writer_tx, writer_rx) = flume::unbounded();
    let (reader_tx, reader_rx) = flume::unbounded();

    let (reader, writer) = connection.split()?;

    let writer_tx_clone = writer_tx.clone();
    let _read_handle = thread::spawn(move || use_reader(reader, reader_tx, writer_tx_clone));
    let _write_handle = thread::spawn(move || use_writer(writer, writer_rx));

    if let Some(freq) = heartbeat {
        let writer_tx_clone = writer_tx.clone();
        let _beat_handle = thread::spawn(move || run_heartbeat(freq, writer_tx_clone));
    }

    Ok((writer_tx, reader_rx))
}

pub fn run_heartbeat(freq: Duration, writer_tx: Sender<ClientMessage>) {
    info!("Start beat");
    // Heart beat should never be less than a second
    assert!(freq.as_millis() > 1000, "Heart beat should never be less than a second");

    loop {
        thread::sleep(freq - jitter());
        if let Err(e) = writer_tx.send(ClientMessage::Heartbeat) {
            error!("Failed to send heartbeat to writer: {}", e);
            break;
        }
    }
}

fn use_reader(
    mut reader: impl std::io::Read + Send + 'static,
    output_tx: Sender<Vec<u8>>,
    writer_tx: Sender<ClientMessage>,
) {
    let mut frame = Frame::empty();

    'read: loop {
        let res = frame.read(&mut reader);

        'msg: loop {
            match res {
                Ok(0) => break 'read,
                Ok(_) => match frame.try_msg() {
                    Ok(None) => break 'msg,
                    Ok(Some(FrameOutput::Heartbeat)) => error!("received a heartbeat on the reader"),
                    Ok(Some(FrameOutput::Message(payload))) => {
                        if let Err(e) = output_tx.send(payload) {
                            error!("Failed to send client message: {}", e);
                        }
                    }
                    Err(Error::MalformedHeader) => {
                        log::error!("Malformed header");
                        break 'read;
                    }
                    Err(_) => unreachable!(),
                },
                Err(e) => {
                    error!("Connection closed: {}", e);
                    break 'read;
                }
            }
        }
    }

    let _ = writer_tx.send(ClientMessage::Quit);
    info!("Client closed (reader)");
}

fn use_writer(mut writer: impl std::io::Write + Send + 'static, rx: Receiver<ClientMessage>) -> Result<()> {
    loop {
        let msg = rx.recv().map_err(|_| Error::ChannelClosed)?;
        match msg {
            ClientMessage::Quit => break,
            ClientMessage::Heartbeat => {
                let beat = &[crate::frame::Header::Heartbeat as u8];
                if let Err(e) = writer.write_all(beat) {
                    error!("Failed to write heartbeat: {}", e);
                    break;
                }
            }
            ClientMessage::Payload(payload) => {
                if let Err(e) = writer.write_all(&payload.0) {
                    error!("Failed to write payload: {}", e);
                    break;
                }
            }
            ClientMessage::Raw(_) => {
                error!("Raw message sent to client. This should not happen. Raw messages are for third party libraries that have their own framing");
                break;
            }
        }
    }

    info!("Client closed (writer)");
    Ok(())
}
