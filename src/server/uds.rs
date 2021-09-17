use std::path::Path;

use tokio::net::UnixListener;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::errors::Result;
use super::{ServerFuture, Server};

/// A unix domain socket server
pub struct UdsServer {
    listener: UnixListener,
}

impl UdsServer {
    /// Create a new uds server given a path.
    ///
    /// ```
    /// # use tinyroute::server::UdsServer;
    /// # fn run() {
    /// let server = UdsServer::bind("/tmp/my-file.sock").expect("fail");
    /// # }
    pub fn bind(addr: impl AsRef<Path>) -> Result<Self> {
        let listener = UnixListener::bind(addr.as_ref())?;

        let inst = Self {
            listener,
        };

        Ok(inst)
    }
}

impl Server for UdsServer {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer> {
        let future = async move {
            Ok(self.listener.accept().await?.0.into_split())
        };

        Box::pin(future)
    }
}
