use tokio::net::TcpListener as TokioListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::errors::Result;
use super::{ServerFuture, Listener};

/// A tcp listener
pub struct TcpListener {
    inner: TokioListener,
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
        let inner = TokioListener::bind(addr).await?;

        let inst = Self {
            inner,
        };

        Ok(inst)
    }
}

impl Listener for TcpListener {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (socket, addr) = self.inner.accept().await?;
            let (reader, writer) = socket.into_split();
            Ok((reader, writer, addr.to_string()))
        };

        Box::pin(future)
    }
}
