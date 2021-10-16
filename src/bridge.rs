//! A [`Bridge`] is a connection between [`crate::Router`]s. 
use std::time::Duration;

use bytes::Bytes;
use log::{error, info};

use crate::agent::{Agent, Message};
use crate::client::{
    connect, ClientMessage, ClientReceiver, ClientSender, TcpClient,
};
use crate::errors::{Error, Result};
use crate::frame::{Frame, FramedMessage};
use crate::{AddressToBytes, ToAddress};

/// An outgoing message from a [`Bridge`]
#[derive(Debug, Clone)]
pub struct BridgeMessageOut(FramedMessage);

#[derive(thiserror::Error, Debug)]
pub enum BridgeError {
    #[error("Failed to reconnect the bridge")]
    Reconnect,

    #[error("Failed to communicate with the underlying connection")]
    Connection,
}

/// An outgoing bridge message, sent through the bridge.
///
/// ```
/// # use tinyroute::Bytes;
/// # fn run<A: tinyroute::ToAddress + tinyroute::AddressToBytes>(agent: tinyroute::Agent<(), A>, bridge_address: A) {
/// let remote_address = b"some_channel".to_vec();
/// let message = b"hello world".to_vec();
/// agent.send_bridged(bridge_address, remote_address.into(), message.into());
/// # }
/// ```
impl BridgeMessageOut {
    pub fn new<T: AddressToBytes>(
        sender: T,
        remote_recipient: Bytes,
        bytes: Bytes,
    ) -> Self {
        let sender_bytes = sender.to_bytes();
        // Sender + | + recipient + | + message
        let mut payload = Vec::with_capacity(
            sender_bytes.len() + 1 + remote_recipient.len() + 1 + bytes.len(),
        );
        payload.extend_from_slice(&remote_recipient);
        payload.push(b'|');
        payload.extend_from_slice(&sender_bytes);
        payload.push(b'|');
        payload.extend_from_slice(&bytes);
        let framed_message = Frame::frame_message(&payload);
        Self(framed_message)
    }
}

/// An incoming bridge message.
///
/// ```
/// use tinyroute::bridge::BridgeMessageIn;
///
/// # use tinyroute::Bytes;
/// # fn run<A: tinyroute::ToAddress + tinyroute::AddressToBytes>(agent: tinyroute::Agent<(), A>, bridge_address: A) {
/// let payload = b"a_remote_address|a message".to_vec();
/// let message = BridgeMessageIn::decode(payload.into()).unwrap();
/// assert_eq!(message.sender.as_ref(), b"a_remote_address");
/// assert_eq!(message.message.as_ref(), b"a message");
/// # }
/// ```
pub struct BridgeMessageIn {
    /// The address of the origin/sender of the message.
    pub sender: Bytes,
    /// The message
    pub message: Bytes,
}

impl BridgeMessageIn {
    pub fn decode(input: Bytes) -> Result<BridgeMessageIn> {
        let sender_address = input
            .iter()
            .take_while(|b| (**b as char) != '|')
            .cloned()
            .collect::<Vec<u8>>();

        if sender_address.len() == input.len() {
            return Err(Error::MissingSender);
        }

        let offset = sender_address.len() + 1;
        if offset >= input.len() {
            return Err(Error::MissingSender);
        }

        let sender_address = Bytes::from(sender_address);
        let bytes = Bytes::from(input[offset..].to_vec());

        Ok(Self { sender: sender_address, message: bytes })
    }
}

#[derive(Debug, Clone)]
pub enum Reconnect {
    Constant(Duration),
    Exponential { seconds: u64, max: Option<u64> },
}

#[derive(Debug, Copy, Clone)]
pub enum Retry {
    Never,
    Forever,
    Count(usize),
}

async fn connect_to(
    addr: impl AsRef<str>,
    reconnect: &mut Reconnect,
    heartbeat: &mut Option<Duration>,
    mut retry: Retry,
) -> Result<(ClientSender, ClientReceiver)> {
    loop {
        match TcpClient::connect(addr.as_ref()).await {
            Ok(c) => {
                info!("Bridge connected");
                break Ok(connect(c, *heartbeat));
            }
            Err(e) => {
                error!("failed to connect. reason: {}", e);
                let sleep_time = match reconnect {
                    Reconnect::Constant(n) => *n,
                    Reconnect::Exponential { seconds, max } => {
                        let secs = match max {
                            Some(max) => seconds.min(max),
                            None => seconds,
                        };
                        let sleep = Duration::from_secs(*secs);
                        *seconds *= 2;
                        *reconnect = Reconnect::Exponential {
                            seconds: *seconds,
                            max: *max,
                        };
                        sleep
                    }
                };
                match retry {
                    Retry::Count(0) => break Err(BridgeError::Reconnect.into()),
                    Retry::Never => break Err(BridgeError::Reconnect.into()),
                    Retry::Count(ref mut n) => *n -= 1,
                    Retry::Forever => {}
                }
                tokio::time::sleep(sleep_time).await;
                info!("retrying...");
            }
        }
    }
}

pub struct Bridge<'addr, A: ToAddress> {
    agent: Agent<BridgeMessageOut, A>,
    addr: &'addr str,
    reconnect: Reconnect,
    heartbeat: Option<Duration>,
    connection: Option<(ClientSender, ClientReceiver)>,
    retry: Retry,
}

impl<'addr, A: ToAddress> Bridge<'addr, A> {
    pub fn new(
        agent: Agent<BridgeMessageOut, A>,
        addr: &'addr str,
        reconnect: Reconnect,
        retry: Retry,
        heartbeat: Option<Duration>,
    ) -> Self {
        Self { agent, addr, reconnect, heartbeat, retry, connection: None }
    }

    async fn reconnect(&mut self) -> Result<(ClientSender, ClientReceiver)> {
        connect_to(
            self.addr,
            &mut self.reconnect,
            &mut self.heartbeat,
            self.retry,
        )
        .await
    }

    pub async fn exec(&mut self) -> Result<Option<Message<BridgeMessageOut, A>>> {
        // Rx here is the incoming data from the network connection.
        // This should never do anything but return `None` once the connection is closed.
        if self.connection.is_none() {
            self.connection = Some(self.reconnect().await?);
        }

        let (bridge_output_tx, rx_client_closed) = self.connection.as_mut().expect("This is okay, because we check the connection above");

        // If the `rx_client` is closed, then reconnect.
        // If the message from the `agent` is invalid, continue and try the next one
        // If the message is okay then return that
        let message = tokio::select! {
            is_closed = rx_client_closed.recv() => {
                let is_closed = is_closed.is_none();
                match is_closed {
                    true => {
                        self.connection = Some(self.reconnect().await?);
                        return Ok(None);
                    }
                    false => return Ok(None), // got a message on the connection rx,
                                              // and we shouldn't be getting anything on there for
                                              // now.
                }
            },
            msg = self.agent.recv() => msg?,
        };

        if let Message::Value(BridgeMessageOut(framed_message), _) = message {
            // if you can read this, know that you are wonderful
            match bridge_output_tx.send(ClientMessage::Payload(framed_message)) {
                Err(_e) => Err(BridgeError::Connection.into()),
                Ok(()) => Ok(None),
            }
        } else {
            Ok(Some(message))
        }
    }
}
