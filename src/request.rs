use std::ops::{Deref, DerefMut};

use flume::Sender;

use crate::agent::AnyMessage;
use crate::error::{Error, Result};

/// A request for data.
///
/// The request is accompanied by a type that is used
/// when reading the request.
/// If the type is incorrect it will produce an `Error::InvalidMessageType` error.
/// ```
/// use tinyroute::error::Error;
/// use tinyroute::request::{Pending, Request};
/// # async fn run(request: Request<Pending>) {
/// match request.read::<String>() {
///     Ok(request) => {
///         let s: &str = &*request;
///         match s {
///             "one" => {
///                 let _ = request.reply_async(1usize).await;
///             }
///             "two" => {
///                 let _ = request.reply_async(2usize).await;
///             }
///             _ => {}
///         }
///     }
///     Err(Error::InvalidMessageType) => {
///         // The message type was incorrect.
///     }
///     Err(e) => panic!("{e}"),
/// }
/// # }
/// ```
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
