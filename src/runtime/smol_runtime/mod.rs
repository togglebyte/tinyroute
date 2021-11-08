pub use smol::Timer;
pub use smol::io::{
    AsyncRead,
    AsyncWrite,
    AsyncWriteExt,
    AsyncReadExt
};
pub use smol::spawn;

mod tcp;
mod uds;

pub use tcp::{TcpListener, TcpClient};
pub use uds::{UdsListener, UdsClient};

pub async fn sleep(time: std::time::Duration) {
    Timer::after(time).await;
}
