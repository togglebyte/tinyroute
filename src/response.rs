use std::marker::PhantomData;

use flume::Receiver;

use crate::agent::AnyMessage;
use crate::error::{Error, Result};

// -----------------------------------------------------------------------------
//     - Response -
// -----------------------------------------------------------------------------
pub struct Response<T: Send + 'static> {
    pub(crate) rx: Receiver<AnyMessage>,
    _p: PhantomData<T>,
}

impl<T: Send + 'static> Response<T> {
    pub(crate) fn new(rx: Receiver<AnyMessage>) -> Self {
        Self {
            rx,
            _p: PhantomData,
        }
    }

    pub async fn recv_async(self) -> Result<T> {
        let any = self.rx.recv_async().await?;

        match any.0.downcast() {
            Ok(val) => Ok(*val),
            Err(_) => Err(Error::InvalidMessageType),
        }
    }

    pub fn recv(self) -> Result<T> {
        let any = self.rx.recv()?;

        match any.0.downcast() {
            Ok(val) => Ok(*val),
            Err(_) => Err(Error::InvalidMessageType),
        }
    }
}
