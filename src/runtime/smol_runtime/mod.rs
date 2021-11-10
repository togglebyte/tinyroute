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

pub use tcp::{TcpListener, TcpClient, TcpConnections};
pub use uds::{UdsListener, UdsClient, UdsConnections};

pub async fn sleep(time: std::time::Duration) {
    Timer::after(time).await;
}
