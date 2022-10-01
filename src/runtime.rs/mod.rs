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
