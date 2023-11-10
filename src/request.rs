use std::ops::{Deref, DerefMut};

use flume::Sender;

use crate::agent::AnyMessage;
use crate::errors::{Error, Result};

pub struct Request<T> {
    pub(crate) tx: Sender<AnyMessage>,
    pub(crate) data: T,
}

pub struct Pending(pub(crate) AnyMessage);

pub struct Received<T>(T);

impl Request<Pending> {
    pub fn read<R: Send + 'static>(self) -> Result<Request<Received<R>>> {
        let Request { data, tx } = self;

        match data.0 .0.downcast() {
            Ok(val) => Ok(Request {
                data: Received(*val),
                tx,
            }),
            Err(_) => Err(Error::InvalidMessageType),
        }
    }
}

impl<R> Request<Received<R>> {
    pub async fn reply_async<T: Send + 'static>(self, data: T) -> Result<()> {
        self.tx
            .send_async(AnyMessage::new(data))
            .await
            .map_err(|_| Error::ChannelClosed)
    }

    pub fn reply<T: Send + 'static>(self, data: T) -> Result<()> {
        self.tx
            .send(AnyMessage::new(data))
            .map_err(|_| Error::ChannelClosed)
    }

    pub fn consume(self) -> R {
        self.data.0
    }
}

impl<T> Deref for Request<Received<T>> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data.0
    }
}

impl<T> DerefMut for Request<Received<T>> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data.0
    }
}
