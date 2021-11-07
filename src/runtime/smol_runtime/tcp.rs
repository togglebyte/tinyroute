pub use smol::io::{
    AsyncRead, 
    AsyncWrite, 
    AsyncWriteExt,
    AsyncReadExt
};
use smol::net::{TcpStream, AsyncToSocketAddrs, TcpListener as SmolTcpListener};

use crate::errors::Result;
use crate::server::{ServerFuture, Listener, ConnectionAddr};
use crate::client::Client;

// -----------------------------------------------------------------------------
//     - Tcp listener -
// -----------------------------------------------------------------------------
/// A tcp listener
pub struct TcpListener {
    inner: SmolTcpListener,
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
        let inner = SmolTcpListener::bind(addr).await?;

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


