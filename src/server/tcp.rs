use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::errors::Result;
use super::{ServerFuture, Server};

/// A tcp server
pub struct TcpServer {
    listener: TcpListener,
}

impl TcpServer {
    /// Create a new tcp server given an address
    ///
    /// ```
    /// # use tinyroute::server::TcpServer;
    /// # async fn run() {
    /// let server = TcpServer::bind("127.0.0.1:5000").await.expect("fail");
    /// # }
    pub async fn bind(addr: &str) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;

        let inst = Self {
            listener,
        };

        Ok(inst)
    }
}

impl Server for TcpServer {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            Ok(self.listener.accept().await?.0.into_split())
        };

        Box::pin(future)
    }
}
