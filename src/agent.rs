//! Agents receive messages from the router, and communicate with other 
//! agents via the [`crate::router::Router`], by sending a message to an [`crate::ToAddress`]
//!
//! Remote agents receive input over the network and requires 
//! a [`crate::agent::Serializer`] and a [`crate::agent::Deserializer`].
//!
//! ## Example: creating a remote agent
//!
//! ```
//! use tinyroute::ToAddress;
//! # use tinyroute::Router;
//! # use tinyroute::agent::{Serializer, Deserializer};
//!
//! #[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash)]
//! enum Address {
//!     Id(usize),
//!     Logger,
//! }
//!
//! impl ToAddress for Address {
//!     // impl to address
//!     # fn from_bytes(bytes: &[u8]) -> Option<Self> {
//!     #     None
//!     # }
//! }
//!
//! # fn run<T, Ser, De> (
//! #     mut router: Router<Address>,
//! #     serializer: Ser,
//! #     deserializer: De
//! # )
//! # where 
//! #     T: Send + 'static + serde::Serialize + serde::de::DeserializeOwned,
//! #     Ser: Serializer<T>,
//! #     De: Deserializer<T>,
//! # {
//! let capacity = 100;
//! let agent = router.new_remote_agent(
//!     capacity,
//!     Address::Id(0),
//!     serializer,
//!     deserializer,
//! );
//! # }
//! ```
//!
//! ## Example: creating a local agent
//!
//! ```
//! # use tinyroute::ToAddress;
//! # use tinyroute::Router;
//! # use tinyroute::agent::{Serializer, Deserializer};
//! # #[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash)]
//! # enum Address {
//! #     Id(usize),
//! #     Logger,
//! # }
//! # impl ToAddress for Address {
//! #     // impl to address
//! #     fn from_bytes(bytes: &[u8]) -> Option<Self> {
//! #         None
//! #     }
//! # }
//! # fn run<T, Ser, De> (
//! #     mut router: Router<Address>,
//! #     serializer: Ser,
//! #     deserializer: De
//! # )
//! # where 
//! #     T: Send + 'static + serde::Serialize + serde::de::DeserializeOwned,
//! #     Ser: Serializer<T>,
//! #     De: Deserializer<T>,
//! # {
//! # let capacity = 100;
//! struct Message {
//!     id: usize,
//!     body: String,
//! }
//!
//! let agent = router.new_local_agent::<Message>(
//!     capacity,
//!     Address::Id(1),
//! );
//! # }
//! ```
//!
//! ## Example: sending a message
//!
//! ```
//! use tinyroute::agent::{Agent, Local};
//! # use tinyroute::ToAddress;
//! # use tinyroute::Router;
//! # #[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash)]
//! # enum Address {
//! #     Id(usize),
//! #     Logger,
//! # }
//! # impl ToAddress for Address {
//! #     // impl to address
//! #     fn from_bytes(bytes: &[u8]) -> Option<Self> {
//! #         None
//! #     }
//! # }
//! # fn run(agent: Agent<Local<String, Address>, Address>) {
//!
//! let message = "hi, how are you".to_string();
//! agent.send(Address::Id(10), message);
//! # }
//! ```
use std::any::Any;
use std::marker::PhantomData;

use bytes::Bytes;
use log::error;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::errors::{Error, Result};
use crate::frame::Frame;
use crate::router::{Channel, RouterMessage, RouterTx, ToAddress};

// -----------------------------------------------------------------------------
//     - Any message -
//     Used to send local messages between agents
// -----------------------------------------------------------------------------
pub(crate) struct AnyMessage(Box<dyn Any + Send + 'static>);

impl AnyMessage {
    pub(crate) fn new<T: Send + 'static>(val: T) -> Self {
        Self(Box::new(val))
    }
}

// -----------------------------------------------------------------------------
//     - Message -
//     This is exposed to the end user
// -----------------------------------------------------------------------------
/// A message received by an [`Agent`]
pub enum Message<T: 'static, A: ToAddress> {
    /// Value containing an instance of T and the address of the sender.
    Value(T, A),
    /// A tracked agent was removed
    AgentRemoved(A),
    /// Close this agent down
    ShutDown,
}

// -----------------------------------------------------------------------------
//     - Agent message -
// -----------------------------------------------------------------------------
pub(crate) enum AgentMsg<A: ToAddress> {
    Message(AnyMessage, A), // A is the address of the sender
    RemoteMessage(Bytes, A),
    AgentRemoved(A),
    ShutDown,
}

impl<A: ToAddress> AgentMsg<A> {
    fn to_local_message<U: 'static>(self) -> Result<Message<U, A>> {
        match self {
            Self::RemoteMessage(_, _) => Err(Error::InvalidMessageConversion),
            Self::AgentRemoved(address) => Ok(Message::AgentRemoved(address)),
            Self::Message(val, sender) => match val.0.downcast() {
                Ok(val) => Ok(Message::Value(*val, sender)),
                Err(_) => Err(Error::InvalidMessageType),
            },
            Self::ShutDown => Ok(Message::ShutDown),
        }
    }
}

// -----------------------------------------------------------------------------
//     - Transports -
// -----------------------------------------------------------------------------
pub struct Local<T: 'static, A: ToAddress> {
    rx: mpsc::Receiver<AgentMsg<A>>,
    _p: PhantomData<T>,
}

impl<T: 'static, A: ToAddress> Local<T, A> {
    pub(crate) fn new(rx: mpsc::Receiver<AgentMsg<A>>) -> Self {
        Self { rx, _p: PhantomData }
    }

    async fn recv(&mut self) -> Result<Message<T, A>> {
        let msg = self.rx.recv().await.ok_or(Error::ChannelClosed)?;
        msg.to_local_message()
    }
}

pub struct Remote<T, A, Ser, De>
where
    T: Send + DeserializeOwned + Serialize + 'static,
    A: ToAddress,
    Ser: Serializer<T>,
    De: Deserializer<T>,
{
    rx: mpsc::Receiver<AgentMsg<A>>,
    serializer: Ser,
    deserializer: De,
    _p: PhantomData<T>,
}

impl<T, A, Ser, De> Remote<T, A, Ser, De>
where
    T: Send + DeserializeOwned + Serialize + 'static,
    A: ToAddress,
    Ser: Serializer<T>,
    De: Deserializer<T>,
{
    pub(crate) fn new(
        rx: mpsc::Receiver<AgentMsg<A>>,
        serializer: Ser,
        deserializer: De,
    ) -> Self {
        Self { rx, serializer, deserializer, _p: PhantomData }
    }

    async fn recv(&mut self) -> Result<Message<T, A>> {
        let msg = self.rx.recv().await.ok_or(Error::ChannelClosed)?;

        match msg {
            AgentMsg::RemoteMessage(bytes, sender) => {
                let val = self.deserializer.deserialize(bytes)?;
                Ok(Message::Value(val, sender))
            }
            _ => msg.to_local_message(),
        }
    }

    fn router_message(
        &mut self,
        recipient: A,
        sender: A,
        val: &T,
    ) -> Result<RouterMessage<A>> {
        let framed_message = {
            let bytes = self.serializer.serialize(val)?;
            Frame::frame_message(&bytes)
        };

        let router_msg = RouterMessage::Message {
            recipient,
            sender,
            msg: AnyMessage::new(framed_message),
        };

        Ok(router_msg)
    }

    fn router_messages<'a>(
        &mut self,
        recipients: impl IntoIterator<Item = &'a A>,
        sender: A,
        val: &T,
    ) -> Result<Vec<RouterMessage<A>>> {
        let framed_message = {
            let bytes = self.serializer.serialize(val)?;
            Frame::frame_message(&bytes)
        };

        let messages = recipients
            .into_iter()
            .map(|recipient| RouterMessage::Message {
                recipient: recipient.clone(),
                sender: sender.clone(),
                msg: AnyMessage::new(framed_message.clone()),
            })
            .collect::<Vec<_>>();

        Ok(messages)
    }
}

pub trait Serializer<T: Serialize> {
    fn serialize(&mut self, val: &T) -> Result<Vec<u8>>;
}

pub trait Deserializer<T: DeserializeOwned> {
    fn deserialize(&mut self, bytes: Bytes) -> Result<T>;
}

// -----------------------------------------------------------------------------
//     - Agent -
// -----------------------------------------------------------------------------
/// An agent receives messages from the [`crate::router::Router`].
pub struct Agent<S, A: ToAddress> {
    pub(crate) router_tx: RouterTx<A>,
    pub(crate) transport: S, //  mpsc::Receiver<AgentMsg<AnyMessage, A>>,
    pub(crate) address: A,
}

impl<S, A: ToAddress> Drop for Agent<S, A> {
    fn drop(&mut self) {
        let _ = self
            .router_tx
            .send(RouterMessage::Unregister(self.address.clone()));
    }
}

impl<S, A: ToAddress> Agent<S, A> {
    pub(crate) fn new(
        router_tx: RouterTx<A>,
        transport: S,
        address: A,
    ) -> Self {
        Self { router_tx, transport, address }
    }

    fn create_agent<T>(
        &self,
        transport_tx: mpsc::Sender<AgentMsg<A>>,
        address: A,
        transport: T,
        channel: Channel,
    ) -> Result<Agent<T, A>> {
        let agent =
            Agent::new(self.router_tx.clone(), transport, address.clone());
        self.router_tx.register_agent(address, transport_tx, channel)?;
        Ok(agent)
    }

    pub fn create_local_agent<T>(
        &self,
        address: A,
        cap: usize,
    ) -> Result<Agent<Local<T, A>, A>> {
        let (tx, transport_rx) = mpsc::channel(cap);
        self.create_agent(tx, address, Local::new(transport_rx), Channel::Local)
    }

    pub fn create_remote_agent<T: Send, De, Ser>(
        &self,
        address: A,
        cap: usize,
        serializer: Ser,
        deserializer: De,
    ) -> Result<Agent<Remote<T, A, Ser, De>, A>>
    where
        T: Send + DeserializeOwned + Serialize + 'static,
        A: ToAddress,
        Ser: Serializer<T>,
        De: Deserializer<T>,
    {
        let (tx, transport_rx) = mpsc::channel(cap);
        self.create_agent(
            tx,
            address,
            Remote::new(transport_rx, serializer, deserializer),
            Channel::Remote,
        )
    }

    pub fn track(&self, address: A) -> Result<()> {
        self.router_tx.send(RouterMessage::Track {
            from: self.address.clone(),
            to: address,
        })?;
        Ok(())
    }

    pub fn reverse_track(&self, address: A) -> Result<()> {
        self.router_tx.send(RouterMessage::Track {
            from: address,
            to: self.address.clone(),
        })?;
        Ok(())
    }

    pub fn address(&self) -> &A {
        &self.address
    }
}

impl<T: Send + 'static, A: ToAddress> Agent<Local<T, A>, A> {
    pub async fn recv(&mut self) -> Result<Message<T, A>> {
        self.transport.recv().await
    }

    pub fn send(&self, recipient: A, message: T) -> Result<()> {
        let router_msg = RouterMessage::Message {
            recipient,
            sender: self.address.clone(),
            msg: AnyMessage::new(message),
        };
        self.router_tx.send(router_msg)?;
        Ok(())
    }
}

impl<T, A, Ser, De> Agent<Remote<T, A, Ser, De>, A>
where
    T: Send + DeserializeOwned + Serialize + 'static,
    A: ToAddress,
    Ser: Serializer<T>,
    De: Deserializer<T>,
{
    pub async fn recv(&mut self) -> Result<Message<T, A>> {
        self.transport.recv().await
    }

    pub fn send(&self, recipient: A, message: T) -> Result<()> {
        let router_msg = RouterMessage::Message {
            recipient,
            sender: self.address.clone(),
            msg: AnyMessage::new(message),
        };
        self.router_tx.send(router_msg)?;
        Ok(())
    }

    pub fn send_remote(
        &mut self,
        recipient: A,
        sender: A,
        val: &T,
    ) -> Result<()> {
        let message = self.transport.router_message(recipient, sender, val)?;
        self.router_tx.send(message)?;
        Ok(())
    }

    pub fn send_remote_many<'a>(
        &mut self,
        recipients: impl IntoIterator<Item = &'a A>,
        sender: A,
        val: &T,
    ) -> Result<()> {
        let recipients: Vec<A> =
            recipients.into_iter().cloned().collect::<Vec<A>>();

        for r in &recipients {
            eprintln!("recipient: {}", r.to_string());
        }

        let messages =
            self.transport.router_messages(&recipients, sender, val)?;

        for message in messages {
            if let Err(e) = self.router_tx.send(message) {
                error!("Failed to send remote message: {:?}", e);
            }
        }
        Ok(())
    }
}
