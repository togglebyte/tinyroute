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
use log::{error, info};
use rand::prelude::*;
use std::time::Duration;

use crate::runtime::{AsyncRead, AsyncWrite, AsyncWriteExt, sleep, spawn};
pub use crate::runtime::TcpClient;

// mod uds;

use crate::errors::{Result, Error};
use crate::frame::{Frame, FrameOutput, FramedMessage};

// pub use uds::UdsClient;

/// Type alias for `tokio::mpsc::Receiver<Vec<u8>>`
pub type ClientReceiver = flume::Receiver<Vec<u8>>;
/// Type alias for `tokio::mpsc::Sender<ClientMessage>`
pub type ClientSender = flume::Sender<ClientMessage>;

/// Client message
#[derive(Debug)]
pub enum ClientMessage {
    /// Shut down the client
    Quit,
    /// Bunch of delicious bytes
    Payload(FramedMessage),
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
        buf.push(b'|');
        buf.extend_from_slice(payload);
        let framed_message = Frame::frame_message(&buf);
        ClientMessage::Payload(framed_message)
    }
}

/// A client connection
pub trait Client {
    /// The reading half of the connection
    type Reader: AsyncRead + Unpin + Send + 'static;
    /// The writing half of the connection
    type Writer: AsyncWrite + Unpin + Send + 'static;

    /// Split the connection into a reader / writer pair
    fn split(self) -> (Self::Reader, Self::Writer);
}

/// Get a [`ClientSender`] and [`ClientReceiver`] pair
pub fn connect(
    connection: impl Client,
    heartbeat: Option<Duration>,
) -> (ClientSender, ClientReceiver) {
    let (writer_tx, writer_rx) = flume::unbounded();
    let (reader_tx, reader_rx) = flume::unbounded();

    let (reader, writer) = connection.split();

    spawn(use_reader(reader, reader_tx, writer_tx.clone()));
    spawn(use_writer(writer, writer_rx));

    if let Some(freq) = heartbeat {
        spawn(run_heartbeat(freq, writer_tx.clone()));
    }

    (writer_tx, reader_rx)
}

async fn run_heartbeat(freq: Duration, writer_tx: flume::Sender<ClientMessage>) {
    info!("Start beat");
    // Heart beat should never be less than a second
    assert!(
        freq.as_millis() > 1000,
        "Heart beat should never be less than a second"
    );

    loop {
        sleep(freq - jitter()).await;
        if let Err(e) = writer_tx.send(ClientMessage::Heartbeat) {
            error!("Failed to send heartbeat to writer: {}", e);
            break;
        }
    }
}

fn jitter() -> Duration {
    let min_ms = 100;
    let max_ms = 2_000;

    let ms = thread_rng().gen_range(min_ms..max_ms);

    Duration::from_millis(ms)
}

async fn use_reader(
    mut reader: impl AsyncRead + Unpin + Send + 'static,
    output_tx: flume::Sender<Vec<u8>>,
    writer_tx: flume::Sender<ClientMessage>,
) {
    let mut frame = Frame::empty();

    'read: loop {
        let res = frame.async_read(&mut reader).await;

        'msg: loop {
            match res {
                Ok(0) => break 'read,
                Ok(_) => match frame.try_msg() {
                    Ok(None) => break 'msg,
                    Ok(Some(FrameOutput::Heartbeat)) => error!("received a heartbeat on the reader"),
                    Ok(Some(FrameOutput::Message(payload))) => drop(output_tx.send(payload)),
                    Err(Error::MalformedHeader) => {}
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

async fn use_writer(
    mut writer: impl AsyncWrite + Unpin + Send + 'static,
    rx: flume::Receiver<ClientMessage>,
) -> Result<()> {
    loop {
        let msg = rx.recv_async().await.map_err(|_| Error::ChannelClosed)?;
        match msg {
            ClientMessage::Quit => break,
            ClientMessage::Heartbeat => {
                let beat = &[crate::frame::Header::Heartbeat as u8];
                if let Err(e) = writer.write_all(beat).await {
                    error!("Failed to write heartbeat: {}", e);
                    break;
                }

            }
            ClientMessage::Payload(payload) => {
                if let Err(e) = writer.write_all(&payload.0).await {
                    error!("Failed to write payload: {}", e);
                    break;
                }
            }
        }
    }

    info!("Client closed (writer)");
    Ok(())
}
