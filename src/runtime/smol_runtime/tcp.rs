pub use smol::io::{
    AsyncRead, 
    AsyncWrite, 
    AsyncWriteExt,
    AsyncReadExt
};
use smol::net::{TcpListener, TcpStream, AsyncToSocketAddrs};

use crate::errors::Result;
use crate::server::{ServerFuture, Connections, ConnectionAddr};
use crate::client::Client;

// -----------------------------------------------------------------------------
//     - Tcp listener -
// -----------------------------------------------------------------------------
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
    type Reader = TcpStream;
    type Writer = TcpStream;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (reader, addr) = self.inner.accept().await?;
            let writer = reader.clone();
            Ok((reader, writer, ConnectionAddr::Tcp(addr)))
        };

        Box::pin(future)
    }
}

// -----------------------------------------------------------------------------
//     - Tcp client -
// -----------------------------------------------------------------------------
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
    pub async fn connect<A: AsyncToSocketAddrs>(addr: A) -> Result<Self> {
        let inner = TcpStream::connect(addr).await?;

        let inst = Self {
            inner
        };

        Ok(inst)
    }
}

impl Client for TcpClient {
    type Reader = TcpStream;
    type Writer = TcpStream;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let reader = self.inner;
        let writer = reader.clone();
        (reader, writer)
    }
}


