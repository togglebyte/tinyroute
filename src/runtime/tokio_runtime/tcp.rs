use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::errors::Result;
use crate::server::{ServerFuture, Connections, ConnectionAddr};
use crate::client::Client;

/// A tcp listener
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

        let inst = Self {
            inner,
        };

        Ok(inst)
    }
}

impl Connections for TcpConnections {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (socket, addr) = self.inner.accept().await?;
            let (reader, writer) = socket.into_split();
            Ok((reader, writer, ConnectionAddr::Tcp(addr)))
        };

        Box::pin(future)
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

        let inst = Self {
            inner
        };

        Ok(inst)
    }
}

impl Client for TcpClient {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (reader, writer) = self.inner.into_split();

        (reader, writer)
    }
}


