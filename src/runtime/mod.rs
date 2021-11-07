// -----------------------------------------------------------------------------
//     - Tokio -
// -----------------------------------------------------------------------------
#[cfg(feature="tokio_rt")]
#[cfg(not(feature="async_std_rt"))]
mod tokio_runtime;

#[cfg(feature="tokio_rt")]
#[cfg(not(feature="async_std_rt"))]
pub use tokio_runtime::*;

// -----------------------------------------------------------------------------
//     - Async STD -
// -----------------------------------------------------------------------------
#[cfg(feature="async_std_rt")]
#[cfg(not(feature="tokio_rt"))]
mod async_std_runtime;

#[cfg(feature="async_std_rt")]
#[cfg(not(feature="tokio_rt"))]
pub use async_std_runtime::*;
