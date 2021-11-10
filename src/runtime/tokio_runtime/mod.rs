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

pub use tcp::{TcpClient, TcpConnections};
pub use uds::{UdsClient, UdsConnections};
pub use tokio::net::{TcpListener, TcpStream};
pub use tokio::net::{UnixListener as UdsListener, UnixStream as UdsStream};

pub fn block_on<T: Send + 'static>(fut: impl Future<Output=T> + Send + 'static) {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the Runtime")
        .block_on(fut);
}
