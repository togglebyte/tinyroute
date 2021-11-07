pub use async_std::task::{spawn, sleep};
pub use async_std::io::{
    Read as AsyncRead, 
    Write as AsyncWrite, 
    WriteExt as AsyncWriteExt,
    ReadExt as AsyncReadExt
};
use async_std::net::{TcpStream, ToSocketAddrs, TcpListener as AsyncStdTcpListener};

use crate::errors::Result;
use crate::server::{ServerFuture, Listener, ConnectionAddr};
use crate::client::Client;

/// A tcp listener
pub struct TcpListener {
    inner: AsyncStdTcpListener,
}

impl TcpListener {
    /// Create a new tcp server given an address
    ///
    /// ```
    /// # use tinyroute::server::TcpListener;
    /// # async fn run() {
    /// let listener = TcpListener::bind("127.0.0.1:5000").await.expect("fail");
    /// # }
    pub async fn bind(addr: &str) -> Result<Self> {
        let inner = AsyncStdTcpListener::bind(addr).await?;

        let inst = Self {
            inner,
        };

        Ok(inst)
    }
}

impl Listener for TcpListener {
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
    type Reader = TcpStream;
    type Writer = TcpStream;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let reader = self.inner;
        let writer = reader.clone();
        (reader, writer)
    }
}
