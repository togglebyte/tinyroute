//! ```
//! use std::io::{Read, Write};
//!
//! use tinyroute::client::{
//!     connect, Client, ClientMessage, ClientReceiver, ClientSender, TcpClient,
//! };
//! use tinyroute::frame::Frame;
//! use tokio::io::{AsyncReadExt, AsyncWriteExt};
//! use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
//! use tokio::sync::mpsc;
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
use std::path::Path;
use std::time::Duration;

use flume::{Receiver, Sender};
use log::{error, info};
use rand::prelude::*;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs, UnixStream};
use tokio::spawn;
use tokio::time::sleep;

use crate::error::{Error, Result};
use crate::frame::{Frame, FrameOutput, FramedMessage};
use crate::ADDRESS_SEP;

/// Type alias for `tokio::mpsc::Receiver<Vec<u8>>`
pub type ClientReceiver = Receiver<Vec<u8>>;
/// Type alias for `tokio::mpsc::Sender<ClientMessage>`
pub type ClientSender = Sender<ClientMessage>;

/// Client message: a message sent by a client.
/// The server will only ever see the payload bytes.
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
    type Reader: AsyncRead + Unpin + Send + 'static;
    /// The writing half of the connection
    type Writer: AsyncWrite + Unpin + Send + 'static;

    /// Split the connection into a reader / writer pair
    fn split(self) -> (Self::Reader, Self::Writer);
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
    pub async fn connect(addr: impl AsRef<Path>) -> Result<Self> {
        let inner = UnixStream::connect(addr).await?;

        let inst = Self { inner };

        Ok(inst)
    }
}

impl Client for UdsClient {
    type Reader = tokio::net::unix::OwnedReadHalf;
    type Writer = tokio::net::unix::OwnedWriteHalf;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (reader, writer) = self.inner.into_split();

        (reader, writer)
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
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let inner = TcpStream::connect(addr).await?;

        let inst = Self { inner };

        Ok(inst)
    }
}

impl Client for TcpClient {
    type Reader = tokio::net::tcp::OwnedReadHalf;
    type Writer = tokio::net::tcp::OwnedWriteHalf;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (reader, writer) = self.inner.into_split();

        (reader, writer)
    }
}

#[cfg(feature = "tls")]
pub mod tls {
    use std::sync::Arc;

    use tokio::io::{self, ReadHalf, WriteHalf};
    use tokio::net::TcpStream;
    use tokio_rustls::client::TlsStream;
    pub use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
    pub use tokio_rustls::rustls::ClientConfig;
    use tokio_rustls::rustls::RootCertStore;
    use tokio_rustls::TlsConnector;

    use super::{Client, TcpClient};
    use crate::error::{Result, TlsError};

    pub struct TlsClient {
        inner: TlsStream<TcpStream>,
    }

    impl TlsClient {
        async fn new(store: RootCertStore, client: TcpClient, domain: &str) -> Result<Self> {
            let config = ClientConfig::builder()
                .with_root_certificates(store)
                .with_no_client_auth();

            Self::with_client_config(Arc::new(config), client, domain).await
        }

        pub async fn with_client_config(
            config: Arc<ClientConfig>,
            client: TcpClient,
            domain: &str,
        ) -> Result<Self> {
            let connector = TlsConnector::from(config);
            let domain = ServerName::try_from(domain).map_err(TlsError::from)?;

            Ok(Self {
                inner: connector.connect(domain.to_owned(), client.inner).await?,
            })
        }
    }

    impl Client for TlsClient {
        type Reader = ReadHalf<TlsStream<TcpStream>>;
        type Writer = WriteHalf<TlsStream<TcpStream>>;

        fn split(self) -> (Self::Reader, Self::Writer) {
            io::split(self.inner)
        }
    }

    pub struct TlsClientBuilder {
        #[cfg(feature = "tls-webpki-roots")]
        webpki_roots: bool,
        #[cfg(feature = "tls-native-certs")]
        native_certs: bool,
        custom_certs: Vec<CertificateDer<'static>>,
    }

    impl TlsClientBuilder {
        pub const fn new() -> Self {
            Self {
                #[cfg(feature = "tls-webpki-roots")]
                webpki_roots: false,
                #[cfg(feature = "tls-native-certs")]
                native_certs: false,
                custom_certs: Vec::new(),
            }
        }

        #[cfg(feature = "tls-webpki-roots")]
        pub const fn with_webpki_roots(mut self) -> Self {
            self.webpki_roots = true;
            self
        }

        #[cfg(feature = "tls-native-certs")]
        pub const fn with_native_certs(mut self) -> Self {
            self.native_certs = true;
            self
        }

        pub fn with_cert(mut self, cert: CertificateDer<'static>) -> Self {
            self.custom_certs.push(cert);
            self
        }

        pub fn with_certs(
            mut self,
            certs: impl IntoIterator<Item = CertificateDer<'static>>,
        ) -> Self {
            self.custom_certs.extend(certs);
            self
        }

        pub async fn build(self, client: TcpClient, domain: &str) -> Result<TlsClient> {
            let mut store = RootCertStore::empty();

            #[cfg(feature = "tls-webpki-roots")]
            {
                if self.webpki_roots {
                    store
                        .roots
                        .extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                }
            }

            #[cfg(feature = "tls-native-certs")]
            {
                if self.native_certs {
                    for cert in rustls_native_certs::load_native_certs().unwrap() {
                        store.add(cert).map_err(TlsError::from)?;
                    }
                }
            }

            for cert in self.custom_certs {
                store.add(cert).map_err(TlsError::from)?;
            }

            TlsClient::new(store, client, domain).await
        }
    }

    impl Default for TlsClientBuilder {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// Get a [`ClientSender`] and [`ClientReceiver`] pair
pub fn connect(
    connection: impl Client,
    heartbeat: Option<Duration>,
) -> (ClientSender, ClientReceiver) {
    let (writer_tx, writer_rx) = flume::unbounded();
    let (reader_tx, reader_rx) = flume::unbounded();

    let (reader, writer) = connection.split();

    let _read_handle = spawn(use_reader(reader, reader_tx, writer_tx.clone()));
    let _write_handle = spawn(use_writer(writer, writer_rx));

    if let Some(freq) = heartbeat {
        let _beat_handle = spawn(run_heartbeat(freq, writer_tx.clone()));
    }

    (writer_tx, reader_rx)
}

pub async fn run_heartbeat(freq: Duration, writer_tx: Sender<ClientMessage>) {
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

pub(crate) fn jitter() -> Duration {
    let min_ms = 100;
    let max_ms = 2_000;
    let ms = thread_rng().gen_range(min_ms..max_ms);
    Duration::from_millis(ms)
}

async fn use_reader(
    mut reader: impl AsyncRead + Unpin + Send + 'static,
    output_tx: Sender<Vec<u8>>,
    writer_tx: Sender<ClientMessage>,
) {
    let mut frame = Frame::empty();

    'read: loop {
        let res = frame.read_async(&mut reader).await;

        'msg: loop {
            match res {
                Ok(0) => break 'read,
                Ok(_) => match frame.try_msg() {
                    Ok(None) => break 'msg,
                    Ok(Some(FrameOutput::Heartbeat)) => {
                        error!("received a heartbeat on the reader")
                    }
                    Ok(Some(FrameOutput::Message(payload))) => {
                        if let Err(e) = output_tx.send_async(payload).await {
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

async fn use_writer(
    mut writer: impl AsyncWrite + Unpin + Send + 'static,
    rx: Receiver<ClientMessage>,
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
            ClientMessage::Raw(_) => {
                error!("Raw message sent to client. This should not happen. Raw messages are for third party libraries that have their own framing");
                break;
            }
        }
    }

    info!("Client closed (writer)");
    Ok(())
}
