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

#[derive(Debug, Clone)]
pub struct BridgeMessageOut(FramedMessage);

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
) -> Option<(ClientSender, ClientReceiver)> {
    let tcp_client = loop {
        match TcpClient::connect(addr.as_ref()).await {
            Ok(c) => break Some(c),
            Err(e) => {
                error!("failed to connect. reason: {:?}", e);
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
                    Retry::Count(0) => break None,
                    Retry::Never => break None,
                    Retry::Count(ref mut n) => *n -= 1,
                    Retry::Forever => {}
                }
                tokio::time::sleep(sleep_time).await;
                info!("retrying...");
            }
        }
    };

    Some(connect(tcp_client?, *heartbeat))
}

// This is a bit silly but I'm pretty tired
enum Action<T> {
    Message(T),
    Reconnect,
    Continue,
}

impl<T> std::fmt::Display for Action<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(_) => write!(f, "Message"),
            Self::Reconnect => write!(f, "Reconnect"),
            Self::Continue => write!(f, "Continue"),
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

    async fn reconnect(&mut self) {
        self.connection = connect_to(
            self.addr,
            &mut self.reconnect,
            &mut self.heartbeat,
            self.retry,
        )
        .await;
    }

    pub async fn run(&mut self) -> Option<Message<BridgeMessageOut, A>> {
        // Rx here is the incoming data from the network connection.
        // This should never do anything but return `None` once the connection is closed.
        // No data should be sent to this guy
        if let None = self.connection {
            self.reconnect().await;
        }

        if self.connection.is_none() {
            return Some(Message::Shutdown);
        }

        let (bridge_output_tx, rx_client_closed) =
            self.connection.as_mut().unwrap();

        let action = tokio::select! {
            is_closed = rx_client_closed.recv() => {
                let is_closed = is_closed.is_none();
                match is_closed {
                    true => Action::Reconnect,
                    false => Action::Continue,
                }
            },
            msg = self.agent.recv() => {
                match msg {
                    Ok(m) => Action::Message(m),
                    Err(e) => {
                        error!("failed to receive message: {:?}", e);
                        Action::Continue
                    }
                }
            }
        };

        eprintln!("--- Action> {}", action);

        let msg = match action {
            Action::Message(m) => {
                eprintln!("> Message");
                m
            }
            Action::Reconnect => {
                eprintln!("> Reconnect...");
                self.reconnect().await;
                // self.connection = Some(connect_to(self.addr, &mut self.reconnect, &mut self.heartbeat).await);
                return None;
            }
            Action::Continue => {
                eprintln!("> Continue");
                return None;
            }
        };

        if let Message::Value(BridgeMessageOut(framed_message), _) = msg {
            // if you can read this, know that you are wonderful
            // let (sender, message): (A, _) = BridgeMessage::decode(framed_message.0).unwrap();

            // // Send framed messages only!
            // let msg = ClientMessage::Payload(framed_message);
            let res = bridge_output_tx
                .send(ClientMessage::Payload(framed_message))
                .await;
            eprintln!("Mesage sent: > {:?}", res.is_ok());
            None
        } else {
            Some(msg)
        }
    }
}
