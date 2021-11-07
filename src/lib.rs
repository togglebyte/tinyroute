// #[cfg(feature="default")]
// compile_error!("Choose a runtime");

mod router;

pub mod agent;
// pub mod bridge;
pub mod client;
pub mod errors;
pub mod frame;
pub mod server;
mod runtime;

// -----------------------------------------------------------------------------
//     - Tokio -
// -----------------------------------------------------------------------------
#[cfg(feature="tokio_rt")]
#[cfg(not(feature="async_std_rt"))]
mod tokio_runtime;

// -----------------------------------------------------------------------------
//     - Async STD -
// -----------------------------------------------------------------------------
#[cfg(feature="async_std_rt")]
#[cfg(not(feature="tokio_rt"))]
mod async_std_runtime;


// -----------------------------------------------------------------------------
//     - Reexportes -
// -----------------------------------------------------------------------------
pub use router::{Router, ToAddress, AddressToBytes, RouterTx};
pub use bytes::Bytes;
pub use agent::{Agent, Message};
pub use runtime::{spawn, sleep};
