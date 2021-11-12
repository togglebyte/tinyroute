pub const ADDRESS_SEP: u8 = b'|';

#[cfg(all(
    not(feature = "tokio-rt"), 
    not(feature = "async-std-rt"), 
    not(feature = "smol-rt")
))]
compile_error!("Specify a runtime: either tokio-rt, async-std-rt or smol-rt");

#[cfg(any(
    feature = "tokio-rt", 
    feature = "async-std-rt", 
    feature = "smol-rt")
)]
macro_rules! tinyroute {
    () => {
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
        pub use router::{AddressToBytes, Router, RouterTx, ToAddress};
        pub use runtime::{block_on, sleep, spawn};
        pub use futures::{select, future::FutureExt};

        pub mod channels {
            pub use flume::{bounded, unbounded, Sender, Receiver};
        }

        pub mod task {
            pub use crate::runtime::JoinHandle;
        }

        pub mod io {
            pub use crate::runtime::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
        }
    };
}

#[cfg(any(
    feature = "tokio-rt",
    feature = "async-std-rt",
    feature = "smol-rt")
)]
tinyroute!();


#[cfg(feature = "websockets")]
pub mod websockets;
