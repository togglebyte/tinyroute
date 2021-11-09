use std::future::Future;

pub use tokio::time::sleep;
pub use tokio::task::{spawn, JoinHandle};
pub use tokio::io::{
    AsyncRead,
    AsyncWrite,
    AsyncWriteExt,
    AsyncReadExt
};

mod tcp;
mod uds;

pub use tcp::{TcpListener, TcpClient};
pub use uds::{UdsListener, UdsClient};

pub fn block_on<T: Send + 'static>(fut: impl Future<Output=T> + Send + 'static) {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the Runtime")
        .block_on(fut);
}
