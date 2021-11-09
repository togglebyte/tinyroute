#[cfg(all(not(feature = "tokio_rt"), not(feature = "async_std_rt"), not(feature = "smol_rt")))]
compile_error!("Specify a runtime: either tokio_rt, async_std_rt or smol_rt");

#[cfg(any(feature = "tokio_rt", feature = "async_std_rt", feature = "smol_rt"))]
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

        pub mod task {
            pub use crate::runtime::JoinHandle;
        }

        pub mod io {
            pub use crate::runtime::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
        }
    };
}

#[cfg(any(feature = "tokio_rt", feature = "async_std_rt", feature = "smol_rt"))]
tinyroute!();
