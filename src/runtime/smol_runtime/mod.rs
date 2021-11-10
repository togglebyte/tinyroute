pub use smol::Timer;
pub use smol::io::{
    AsyncRead,
    AsyncWrite,
    AsyncWriteExt,
    AsyncReadExt
};
pub use smol::{spawn, block_on, Task as JoinHandle};

mod tcp;
mod uds;

pub use tcp::{TcpClient, TcpConnections};
pub use uds::{UdsClient, UdsConnections};
pub use smol::net::unix::{UnixListener as UdsListener, UnixStream as UdsStream};
pub use smol::net::{TcpListener, TcpStream};

pub async fn sleep(time: std::time::Duration) {
    Timer::after(time).await;
}
