//! Creating a server
//!
//! ```
//! use tinyroute::server::{Server, TcpConnections};
//!
//! #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
//! struct Address(usize);
//!
//! # impl tinyroute::ToAddress for Address {
//! #   fn from_bytes(_: &[u8]) -> Option<Self> { None }
//! # }
//! # async fn run(mut router: tinyroute::Router<Address>) {
//! let tcp_listener = TcpConnections::bind("127.0.0.1:5000").await.unwrap();
//! let server_agent = router.new_agent(None, Address(0)).unwrap();
//! let mut server = Server::new(tcp_listener, server_agent);
//! let mut id = 0;
//!
//! while let Ok(connection) = server.next(Address(id), None, Some(1024)).await {
//!     id += 1;
//! }
//! # }
//! ```
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use log::error;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
pub use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::spawn;
use tokio::time::sleep;

use crate::agent::{Agent, Message};
use crate::errors::{Error, Result};
use crate::frame::{Frame, FrameOutput, FramedMessage};
use crate::router::{RouterMessage, RouterTx, ToAddress};
use crate::ADDRESS_SEP;

/// A unix domain socket server
pub struct UdsConnections {
    inner: UnixListener,
}

impl UdsConnections {
    /// Create a new uds server given a path.
    ///
    /// ```
    /// # use tinyroute::server::UdsConnections;
    /// # async fn run() {
    /// let listener = UdsConnections::bind("/tmp/my-file.sock").await.expect("failed to create socket");
    /// # }
    pub async fn bind(addr: impl AsRef<Path>) -> Result<Self> {
        let inner = UnixListener::bind(addr.as_ref())?;

        let inst = Self { inner };

        Ok(inst)
    }
}

impl Connections for UdsConnections {
    type Reader = tokio::net::unix::OwnedReadHalf;
    type Writer = tokio::net::unix::OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (socket, _) = self.inner.accept().await?;
            let (reader, writer) = socket.into_split();
            Ok((reader, writer, ConnectionAddr::Uds))
        };

        Box::pin(future)
    }
}

/// Wraps a tcp listener and provides reader, writer and address when accepting
/// incoming connections.
///
/// Connections should be used together with an agent and a [`crate::server::Server`]
pub struct TcpConnections {
    inner: TcpListener,
}

impl TcpConnections {
    /// Create a new tcp server given an address
    ///
    /// ```
    /// # use tinyroute::server::TcpConnections;
    /// # async fn run() {
    /// let listener = TcpConnections::bind("127.0.0.1:5000").await.expect("fail");
    /// # }
    pub async fn bind(addr: &str) -> Result<Self> {
        let inner = TcpListener::bind(addr).await?;

        let inst = Self { inner };

        Ok(inst)
    }
}

impl Connections for TcpConnections {
    type Reader = tokio::net::tcp::OwnedReadHalf;
    type Writer = tokio::net::tcp::OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (socket, addr) = self.inner.accept().await?;
            let (reader, writer) = socket.into_split();
            Ok((reader, writer, ConnectionAddr::Tcp(addr)))
        };

        Box::pin(future)
    }
}

#[cfg(feature = "tls")]
pub mod tls {
    use std::sync::Arc;

    use tokio::io::{ReadHalf, WriteHalf};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::rustls::ServerConfig;
    pub use tokio_rustls::rustls::{Certificate, PrivateKey};
    use tokio_rustls::server::TlsStream;
    use tokio_rustls::TlsAcceptor;

    use super::{ConnectionAddr, Connections};
    use crate::errors::{Result, TlsError};

    pub struct TlsConnections {
        acceptor: TlsAcceptor,
        inner: TcpListener,
    }

    impl TlsConnections {
        pub fn new(
            inner: TcpListener,
            cert_chain: Vec<Certificate>,
            key: PrivateKey,
        ) -> Result<Self> {
            Ok(Self::new_with_config(
                inner,
                Arc::new(
                    ServerConfig::builder()
                        .with_safe_defaults()
                        .with_no_client_auth()
                        .with_single_cert(cert_chain, key)
                        .map_err(TlsError::from)?,
                ),
            ))
        }

        pub fn new_with_config(inner: TcpListener, config: Arc<ServerConfig>) -> Self {
            Self {
                acceptor: config.into(),
                inner,
            }
        }
    }

    impl Connections for TlsConnections {
        type Reader = ReadHalf<TlsStream<TcpStream>>;
        type Writer = WriteHalf<TlsStream<TcpStream>>;

        fn accept(&mut self) -> super::ServerFuture<'_, Self::Reader, Self::Writer> {
            Box::pin(async move {
                let (stream, peer_addr) = self.inner.accept().await?;
                let stream = self.acceptor.accept(stream).await.unwrap();
                let (reader, writer) = tokio::io::split(stream);

                Ok((reader, writer, ConnectionAddr::Tcp(peer_addr)))
            })
        }
    }
}

/// Client payload.
/// Access the bytes through `self.data()`
#[derive(Debug, Clone)]
pub(crate) struct Payload {
    inner: Vec<u8>,
    offset: usize,
}

impl Payload {
    pub(crate) fn new(offset: usize, inner: Vec<u8>) -> Self {
        Self { inner, offset }
    }

    /// Access the inner data of the payload.
    /// This is the data void of the address
    pub(crate) fn data(&self) -> &[u8] {
        &self.inner[self.offset..]
    }
}

/// Some kind of listener
pub trait Connections: Sync {
    /// The reading half of the connection
    type Reader: AsyncRead + Unpin + Send + 'static;
    /// The writing half of the connection
    type Writer: AsyncWrite + Unpin + Send + 'static;

    // Accepts &self as arg
    // Returns a pinned boxed future, where
    // * any reference has to live for at least as long as &self,
    // * and it has to be valid to send this across thread boundaries
    //
    // We need the `Send` part because tokio::spawn might put this on another thread.
    // We need the life time because the thing we return can not hold a reference to
    // anything on &self that might be dropped before self.
    /// Accept incoming connections.
    /// If the `Timeout` is set, this means that the server will close and remove
    /// the connection if no message has been received within the given duration.
    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer>;
}

/// Because writing this entire trait malarkey is messy!
pub type ServerFuture<'a, T, U> =
    Pin<Box<dyn Future<Output = Result<(T, U, ConnectionAddr)>> + Send + 'a>>;

/// Accept incoming connections and provide agents as an abstraction.
///
/// ```
/// use tinyroute::server::{Server, TcpConnections};
///
/// #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
/// struct Address(usize);
///
/// # impl tinyroute::ToAddress for Address {
/// #   fn from_bytes(_: &[u8]) -> Option<Self> { None }
/// # }
/// # async fn run(mut router: tinyroute::Router<Address>) {
/// let tcp_listener = TcpConnections::bind("127.0.0.1:5000").await.unwrap();
/// let server_agent = router.new_agent(None, Address(0)).unwrap();
/// let mut server = Server::new(tcp_listener, server_agent);
/// let mut id = 0;
///
/// while let Ok(connection) = server.next(Address(id), None, Some(1024)).await {
///     id += 1;
/// }
/// # }
/// ```
pub struct Server<C: Connections, A: Sync + ToAddress> {
    server: C,
    server_agent: Agent<(), A>,
}

impl<C: Connections, A: Sync + ToAddress> Server<C, A> {
    pub fn new(server: C, server_agent: Agent<(), A>) -> Self {
        Self {
            server,
            server_agent,
        }
    }

    /// Produce a [`Connection`]
    pub async fn next(
        &mut self,
        connection_address: A,
        timeout: Option<Duration>,
        cap: Option<usize>,
    ) -> Result<Connection<A, <C as Connections>::Writer>> {
        let (reader, writer, socket_addr) = tokio::select! {
            _ = self.server_agent.recv() => return Err(Error::ChannelClosed),
            con = self.server.accept() => con?,
        };

        let agent = self
            .server_agent
            .new_agent(cap, connection_address.clone())
            .await?;

        // Spawn the reader
        let _reader_handle = spawn(spawn_reader(
            reader,
            connection_address,
            socket_addr,
            self.server_agent.router_tx.clone(),
            timeout,
        ));

        Ok(Connection::new(agent, writer))
    }

    /// Consume the [`Server]` and listening for new connections.
    /// Each new connection is sent to it's own task.
    ///
    /// This is useful when letting the router handle the connections,
    /// and all messages are passed as [`Message::RemoteMessage`].
    ///
    /// The `cap` is the message capacity for the [`crate::Agent`] associated with the connection.
    /// If the capacity is `None` an unbounded receiver is created.
    pub async fn run<F>(
        mut self,
        timeout: Option<Duration>,
        cap: Option<usize>,
        mut f: F,
    ) -> Result<()>
    where
        F: FnMut() -> A,
    {
        while let Ok(mut connection) = self.next((f)(), timeout, cap).await {
            spawn(async move {
                loop {
                    match connection.recv().await {
                        Ok(Some(Message::Shutdown)) => break,
                        Err(e) => {
                            error!("Connection error: {}", e);
                            break;
                        }
                        _ => (),
                    }
                }
            });
        }
        Ok(())
    }
}

pub async fn handle_payload<A: ToAddress>(
    bytes: Vec<u8>,
    router_tx: &RouterTx<A>,
    socket_addr: ConnectionAddr,
    sender: A,
) -> bool {
    let address = bytes
        .iter()
        .cloned()
        .take_while(|b| *b != ADDRESS_SEP)
        .collect::<Vec<u8>>();

    // return in the event of the index being
    // larger than the payload it self
    let index = address.len() + 1;
    if index >= bytes.len() {
        return true;
    }

    let address = match A::from_bytes(&address) {
        Some(a) => a,
        None => return true,
    };

    let payload = Payload::new(index, bytes);
    let bytes = Bytes::from(payload.data().to_vec());

    match router_tx
        .send(RouterMessage::RemoteMessage {
            bytes,
            sender,
            host: socket_addr,
            recipient: address,
        })
        .await
    {
        Ok(_) => true,
        Err(e) => {
            error!("failed to send message to router: {}", e);
            false
        }
    }
}

async fn spawn_reader<A, R>(
    mut reader: R,
    sender: A,
    socket_addr: ConnectionAddr,
    router_tx: RouterTx<A>,
    timeout: Option<Duration>,
) where
    R: AsyncRead + Unpin,
    A: ToAddress,
{
    let mut frame = Frame::empty();
    loop {
        let read = async {
            let res = frame.read_async(&mut reader).await;

            'msg: loop {
                match res {
                    Err(e) => {
                        error!("failed to read from the socket. reason: {:?}", e);
                        break 'msg false;
                    }
                    Ok(0) => break 'msg false,
                    Ok(_) => match frame.try_msg() {
                        Ok(None) => break 'msg true,
                        Err(e) => {
                            error!("invalid payload. {}", e);
                            break 'msg false;
                        }
                        Ok(Some(FrameOutput::Heartbeat)) => continue,
                        Ok(Some(FrameOutput::Message(msg))) => {
                            match handle_payload(
                                msg,
                                &router_tx,
                                socket_addr.clone(),
                                sender.clone(),
                            )
                            .await
                            {
                                true => continue,
                                false => break 'msg false,
                            }
                        }
                    },
                }
            }
        };

        let restart = match timeout {
            Some(timeout) => {
                tokio::select! {
                    _ = sleep(timeout) => false,
                    restart = read =>  restart ,
                }
            }
            None => read.await,
        };

        if !restart {
            break;
        }
    }

    // Shutdown the agent
    if let Err(e) = router_tx.send(RouterMessage::Shutdown(sender)).await {
        error!("failed to shutdown agent: {}", e);
    }
}

// -----------------------------------------------------------------------------
//     - Connection -
// -----------------------------------------------------------------------------
pub struct Connection<A, W>
where
    A: ToAddress,
    W: AsyncWrite,
{
    agent: Agent<FramedMessage, A>,
    writer: W,
}

impl<A, W> Connection<A, W>
where
    A: ToAddress,
    W: AsyncWrite + Unpin,
{
    pub fn new(agent: Agent<FramedMessage, A>, writer: W) -> Self {
        Self { agent, writer }
    }

    pub async fn recv(&mut self) -> Result<Option<Message<FramedMessage, A>>> {
        let msg = self.agent.recv().await?;
        match msg {
            Message::Value(framed_message, _) => {
                self.writer.write_all(&framed_message.0).await?;
                Ok(None)
            }
            _ => Ok(Some(msg)),
        }
    }
}

// -----------------------------------------------------------------------------
//     - Connection address -
// -----------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub enum ConnectionAddr {
    Tcp(std::net::SocketAddr),
    Uds,
}

impl Display for ConnectionAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(addr) => write!(f, "{}", addr),
            Self::Uds => write!(f, "Uds"),
        }
    }
}
