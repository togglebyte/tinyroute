pub const ADDRESS_SEP: u8 = b'|';

mod router;

pub mod agent;
pub mod bridge;
pub mod client;
pub mod client_sync;
pub mod errors;
pub mod frame;
pub mod server;

// -----------------------------------------------------------------------------
//     - Reexportes -
// -----------------------------------------------------------------------------
pub use agent::{Agent, Message};
pub use bytes::Bytes;
pub use router::{Router, RouterTx, ToAddress};

pub mod channels {
    pub use flume::{bounded, unbounded, Receiver, Sender};
}
