use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use super::Client;
use crate::errors::Result;

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

