use std::path::Path;

use tokio::net::UnixListener as TokioListener;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::errors::Result;
use crate::server::{ServerFuture, Listener, ConnectionAddr};
use crate::client::Client;

/// A unix domain socket server
pub struct UdsListener {
    inner: TokioListener,
}

impl UdsListener {
    /// Create a new uds server given a path.
    ///
    /// ```
    /// # use tinyroute::server::UdsListener;
    /// # fn run() {
    /// let listener = UdsListener::bind("/tmp/my-file.sock").expect("fail");
    /// # }
    pub fn bind(addr: impl AsRef<Path>) -> Result<Self> {
        let inner = TokioListener::bind(addr.as_ref())?;

        let inst = Self {
            inner,
        };

        Ok(inst)
    }
}

impl Listener for UdsListener {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (socket, _) = self.inner.accept().await?;
            let (reader, writer) = socket.into_split();
            Ok((reader, writer, ConnectionAddr::Uds))
        };

        Box::pin(future)
    }
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

        let inst = Self {
            inner
        };

        Ok(inst)
    }
}

impl Client for UdsClient {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (reader, writer) = self.inner.into_split();

        (reader, writer)
    }
}


