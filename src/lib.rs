pub const ADDRESS_SEP: u8 = b'|';

mod router;
mod runtime;

pub mod agent;
pub mod bridge;
pub mod client;
pub mod errors;
pub mod frame;
pub mod server;

// -----------------------------------------------------------------------------
//     - Reexportes -
// -----------------------------------------------------------------------------
pub use agent::{Agent, Message};
pub use bytes::Bytes;
pub use futures::{future::FutureExt, select};
pub use router::{Router, RouterTx, ToAddress};

pub mod channels {
    pub use flume::{bounded, unbounded, Receiver, Sender};
}

// pub mod task {
//     pub use crate::runtime::JoinHandle;
// }

// pub mod io {
//     pub use crate::runtime::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
// }
