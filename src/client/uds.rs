use std::path::Path;

use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use super::Client;
use crate::errors::Result;

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

