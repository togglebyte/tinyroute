//! Agents receive messages from the router, and communicate with other
//! Agents via the [`crate::router::Router`], by sending a message to an [`crate::ToAddress`]
//!
//! Remote agents receive input over the network.
//!
//! ## Example: creating an agent
//!
//! ```
//! use tinyroute::ToAddress;
//! # use tinyroute::Router;
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
//! # fn run<T> (
//! #     mut router: Router<Address>,
//! # )
//! # where
//! #     T: Send + 'static
//! # {
//! let capacity = 100;
//! let agent = router.new_agent::<T>(
//!     capacity,
//!     Address::Id(0),
//! );
//! # }
//! ```
//!
//! ## Example: sending a message
//!
//! ```
//! use tinyroute::agent::Agent;
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
//! # fn run(agent: Agent<String, Address>) {
//!
//! let message = "hi, how are you".to_string();
//! agent.send(Address::Id(10), message);
//! # }
//! ```
//!
//! ## Example: receiving a message
//!
//! ```
//! use tinyroute::agent::{Agent, Message};
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
//! # async fn run(mut agent: Agent<String, Address>) {
//!
//! while let Ok(msg) = agent.recv().await {
//!     match msg {
//!         Message::Value(value, sender) => println!("message received: {} from {}", value, sender.to_string()),
//!         Message::RemoteMessage(bytes, sender) => println!("{} sent {} bytes", sender.to_string(), bytes.len()),
//!         Message::Shutdown => break,
//!         Message::AgentRemoved(address) => println!("Agent {} was removed, and we care", address.to_string()),
//!     }
//! }
//! # }
//! ```
use std::any::Any;
use std::fmt::{Display, Formatter, Result as DisplayResult};
use std::marker::PhantomData;

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::bridge::BridgeMessageOut;
use crate::errors::{Error, Result};
use crate::router::{RouterMessage, RouterTx, ToAddress, AddressToBytes};

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
    /// Bytes received from a socket
    RemoteMessage(Bytes, A),
    /// Value containing an instance of T and the address of the sender.
    Value(T, A),
    /// A tracked agent was removed
    AgentRemoved(A),
    /// Close this agent down
    Shutdown,
}

impl<T: Clone + 'static, A: ToAddress> Clone for Message<T, A> {
    fn clone(&self) -> Self {
        match self {
            Self::RemoteMessage(bytes, addr) => {
                Self::RemoteMessage(bytes.clone(), addr.clone())
            }
            Self::Value(val, addr) => Self::Value(val.clone(), addr.clone()),
            Self::AgentRemoved(addr) => Self::AgentRemoved(addr.clone()),
            Self::Shutdown => Self::Shutdown,
        }
    }
}

impl<T: Display + 'static, A: ToAddress> Display for Message<T, A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> DisplayResult {
        match self {
            Self::Value(val, sender) => {
                write!(f, "{} > Value<{}>", sender.to_string(), val)
            }
            Self::RemoteMessage(bytes, sender) => write!(
                f,
                "Remote({}) > Bytes({})",
                sender.to_string(),
                bytes.len()
            ),
            Self::AgentRemoved(addr) => {
                write!(f, "AgentRemoved<{}>", addr.to_string())
            }
            Self::Shutdown => write!(f, "Shutdown"),
        }
    }
}

// -----------------------------------------------------------------------------
//     - Agent message -
// -----------------------------------------------------------------------------
pub(crate) enum AgentMsg<A: ToAddress> {
    Message(AnyMessage, A), // A is the address of the sender
    RemoteMessage(Bytes, A),
    AgentRemoved(A),
    Shutdown,
}

impl<A: ToAddress> AgentMsg<A> {
    fn to_local_message<U: 'static>(self) -> Result<Message<U, A>> {
        match self {
            Self::RemoteMessage(bytes, sender) => {
                Ok(Message::RemoteMessage(bytes, sender))
            }
            Self::AgentRemoved(address) => Ok(Message::AgentRemoved(address)),
            Self::Message(val, sender) => match val.0.downcast() {
                Ok(val) => Ok(Message::Value(*val, sender)),
                Err(_) => Err(Error::InvalidMessageType),
            },
            Self::Shutdown => Ok(Message::Shutdown),
        }
    }
}

// -----------------------------------------------------------------------------
//     - Agent -
// -----------------------------------------------------------------------------
/// An agent receives messages from the [`crate::router::Router`].
pub struct Agent<T, A: ToAddress> {
    pub(crate) router_tx: RouterTx<A>,
    // pub(crate) transport: S, //  mpsc::Receiver<AgentMsg<AnyMessage, A>>,
    pub(crate) address: A,
    rx: mpsc::Receiver<AgentMsg<A>>,
    _p: PhantomData<T>,
}

impl<S, A: ToAddress> Drop for Agent<S, A> {
    fn drop(&mut self) {
        let _ = self
            .router_tx
            .send(RouterMessage::Unregister(self.address.clone()));
    }
}

impl<T: Send + 'static, A: ToAddress> Agent<T, A> {
    pub(crate) fn new(
        router_tx: RouterTx<A>,
        address: A,
        rx: mpsc::Receiver<AgentMsg<A>>,
    ) -> Self {
        Self { router_tx, rx, address, _p: PhantomData }
    }

    pub async fn new_agent<U: Send + 'static>(
        &self,
        address: A,
        cap: usize,
    ) -> Result<Agent<U, A>> {
        let (transport_tx, transport_rx) = mpsc::channel(cap);
        let agent =
            Agent::new(self.router_tx.clone(), address.clone(), transport_rx);
        self.router_tx.register_agent(address, transport_tx).await?;
        Ok(agent)
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

    pub async fn recv(&mut self) -> Result<Message<T, A>> {
        let msg = self.rx.recv().await.ok_or(Error::ChannelClosed)?;
        msg.to_local_message()
    }

    // // TODO: is this even used? Remove if it's not used by 2021-09-25
    // pub async fn recv_routed(&mut self) -> Result<Message<T, A>> {
    //     let msg = self.rx.recv().await.ok_or(Error::ChannelClosed)?;
    //     if let AgentMsg::RemoteMessage(bytes, address) =  msg {
    //         // translate the remote message to a routed message
    //         todo!("convert the remote to a RoutedMessage")
    //     } else {
    //         msg.to_local_message()
    //     }
    // }

    pub fn send<U: Send + 'static>(
        &self,
        recipient: A,
        message: U,
    ) -> Result<()> {
        let router_msg = RouterMessage::Message {
            recipient,
            sender: self.address.clone(),
            msg: AnyMessage::new(message),
        };
        self.router_tx.send(router_msg)?;
        Ok(())
    }

    /// This is used for debugging, to print 
    /// the current list of registered channels on a router.
    pub fn print_channels(&self) {
        let _ = self.router_tx.send(RouterMessage::PrintChannels);
    }

    pub fn send_remote(&self, recipient: A, bytes: Bytes) -> Result<()> {
        let router_msg = RouterMessage::RemoteMessage {
            recipient,
            sender: self.address.clone(),
            bytes,
        };
        self.router_tx.send(router_msg)?;
        Ok(())
    }

    pub fn shutdown(&self) {
        let router_msg = RouterMessage::Shutdown(self.address.clone());
        let _ = self.router_tx.send(router_msg);
    }
}


impl<T: Send + 'static, A: ToAddress + AddressToBytes> Agent<T, A> {
    pub fn send_bridged(
        &self,
        bridge_address: A,
        remote: Bytes,
        message: Bytes,
    ) -> Result<()> {
        let msg = BridgeMessageOut::new(self.address.clone(), remote, message);
        let router_msg = RouterMessage::Message {
            sender: self.address.clone(),
            recipient: bridge_address,
            msg: AnyMessage::new(msg),
        };
        self.router_tx.send(router_msg)?;
        Ok(())
    }
}
