use async_std::net::{TcpStream, TcpListener as AsyncStdTcpListener};

use crate::errors::Result;
use super::{ServerFuture, Listener, ConnectionAddr};

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

