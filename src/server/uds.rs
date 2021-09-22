use std::path::Path;

use tokio::net::UnixListener as TokioListener;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::errors::Result;
use super::{ServerFuture, Listener};

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
            let (socket, addr) = self.inner.accept().await?;
            let (reader, writer) = socket.into_split();
            let addr = addr
                .as_pathname()
                .and_then(Path::to_str)
                .unwrap_or_else(|| "[no address]")
                .to_string();
            Ok((reader, writer, addr))
        };

        Box::pin(future)
    }
}
