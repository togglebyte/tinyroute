use std::path::Path;

use async_std::os::unix::net::{UnixListener as AsyncStdListener, UnixStream};

use crate::errors::Result;
use crate::server::{ServerFuture, Listener, ConnectionAddr};
use crate::client::Client;

/// A unix domain socket server
pub struct UdsListener {
    inner: AsyncStdListener,
}

impl UdsListener {
    /// Create a new uds server given a path.
    ///
    /// ```
    /// # use tinyroute::server::UdsListener;
    /// # async fn run() {
    /// let listener = UdsListener::bind("/tmp/my-file.sock").await.expect("failed to crate socket");
    /// # }
    pub async fn bind(addr: impl AsRef<Path>) -> Result<Self> {
        let inner = AsyncStdListener::bind(addr.as_ref()).await?;

        let inst = Self {
            inner,
        };

        Ok(inst)
    }
}

impl Listener for UdsListener {
    type Reader = UnixStream;
    type Writer = UnixStream;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            let (reader, _) = self.inner.accept().await?;
            let writer = reader.clone();
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
        let inner = UnixStream::connect(addr.as_ref()).await?;

        let inst = Self {
            inner
        };

        Ok(inst)
    }
}

impl Client for UdsClient {
    type Reader = UnixStream;
    type Writer = UnixStream;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let reader = self.inner;
        let writer = reader.clone();

        (reader, writer)
    }
}

