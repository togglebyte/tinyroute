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
//!     Some(capacity),
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
//!         Message::Fetch(_) => println!("fetch received"),
//!         Message::RemoteMessage { bytes, sender, host } => println!("{}@{} sent {} bytes", sender.to_string(), host, bytes.len()),
//!         Message::Shutdown => break,
//!         Message::AgentRemoved(address) => println!("Agent {} was removed, and we care", address.to_string()),
//!     }
//! }
//! # }
//! ```
use std::any::Any;
use std::fmt::{Debug, Display, Formatter, Result as DisplayResult};
use std::marker::PhantomData;

use bytes::Bytes;

use crate::bridge::BridgeMessageOut;
use crate::errors::{Error, Result};
use crate::frame::Frame;
use crate::router::{Request, RouterMessage, RouterTx, ToAddress};
use crate::server::ConnectionAddr;

// -----------------------------------------------------------------------------
//     - Any message -
//     Used to send local messages between agents
// -----------------------------------------------------------------------------
pub(crate) struct AnyMessage(pub(crate) Box<dyn Any + Send + 'static>);

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
    /// Request data from this agent
    Fetch(Request),
    /// Bytes received from a socket
    RemoteMessage { bytes: Bytes, sender: A, host: ConnectionAddr },
    /// A tracked agent was removed
    AgentRemoved(A),
    /// Close this agent down.
    Shutdown,
}

impl<T: Clone + 'static, A: ToAddress> Clone for Message<T, A> {
    fn clone(&self) -> Self {
        match self {
            Self::Value(val, addr) => Self::Value(val.clone(), addr.clone()),
            Self::Fetch(_) => panic!("Can not clone fetch requests"),
            Self::RemoteMessage { bytes, sender, host } => {
                Self::RemoteMessage {
                    bytes: bytes.clone(),
                    sender: sender.clone(),
                    host: host.clone(),
                }
            }
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
            Self::Fetch(_) => write!(f, "<Fetch>"),
            Self::RemoteMessage { bytes, sender, host } => write!(
                f,
                "{}@{} > Bytes({})",
                sender.to_string(),
                host,
                bytes.len()
            ),
            Self::AgentRemoved(addr) => {
                write!(f, "AgentRemoved<{}>", addr.to_string())
            }
            Self::Shutdown => write!(f, "Shutdown"),
        }
    }
}

impl<T: Debug + 'static, A: ToAddress> Debug for Message<T, A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> DisplayResult {
        match self {
            Self::Value(val, sender) => {
                write!(f, "{} > Value<{:?}>", sender.to_string(), val)
            }
            Self::Fetch(_) => {
                write!(f, "<Fetch>")
            }
            Self::RemoteMessage { bytes, sender, host } => write!(
                f,
                "{}@{} > Bytes({})",
                sender.to_string(),
                host,
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
pub(crate) enum AgentMsg<A> {
    Message(AnyMessage, A), // A is the address of the sender
    Fetch(Request),
    RemoteMessage(Bytes, A, ConnectionAddr),
    AgentRemoved(A),
    Shutdown,
}

impl<A: ToAddress> AgentMsg<A> {
    fn into_local_message<U: 'static>(self) -> Result<Message<U, A>> {
        match self {
            Self::Message(val, sender) => match val.0.downcast() {
                Ok(val) => Ok(Message::Value(*val, sender)),
                Err(_) => Err(Error::InvalidMessageType),
            },
            Self::Fetch(request) => Ok(Message::Fetch(request)),
            Self::RemoteMessage(bytes, sender, host) => {
                Ok(Message::RemoteMessage { bytes, sender, host })
            }
            Self::AgentRemoved(address) => Ok(Message::AgentRemoved(address)),
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
    pub(crate) address: A,
    rx: flume::Receiver<AgentMsg<A>>,
    _p: PhantomData<T>,
}

impl<S, A: ToAddress> Drop for Agent<S, A> {
    fn drop(&mut self) {
        let _ = self
            .router_tx
            .send_sync(RouterMessage::Unregister(self.address.clone()));
    }
}

impl<T: Send + 'static, A: ToAddress> Agent<T, A> {
    pub(crate) fn new(
        router_tx: RouterTx<A>,
        address: A,
        rx: flume::Receiver<AgentMsg<A>>,
    ) -> Self {
        Self { router_tx, rx, address, _p: PhantomData }
    }

    /// Create a new agent and register it with the router.
    ///
    /// ```
    /// # use tinyroute::{Agent, ToAddress};
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// # pub enum Address {
    /// #     NewAgent,
    /// # }
    /// #
    /// # impl ToAddress for Address {
    /// #     fn from_bytes(bytes: &[u8]) -> Option<Address> {
    /// #         match bytes {
    /// #             _ => None
    /// #         }
    /// #     }
    /// # }
    /// # async fn run(agent: Agent<(), Address>) {
    /// let new_agent = agent.new_agent::<()>(None, Address::NewAgent).await.unwrap();
    /// # }
    /// ```
    pub async fn new_agent<U: Send + 'static>(
        &self,
        cap: Option<usize>,
        address: A,
    ) -> Result<Agent<U, A>> {
        let (transport_tx, transport_rx) = match cap {
            Some(cap) => flume::bounded(cap),
            None => flume::unbounded(),
        };
        let agent =
            Agent::new(self.router_tx.clone(), address.clone(), transport_rx);
        self.router_tx.register_agent(address, transport_tx).await?;
        Ok(agent)
    }

    pub fn router_tx(&self) -> RouterTx<A> {
        self.router_tx.clone()
    }

    /// Track an agent (or more precisely an address).
    /// If the address is unregistered, the tracking agent will
    /// receive a `Message::AgentRemoved(tracked_address)`.
    pub async fn track(&self, address: A) -> Result<()> {
        self.router_tx
            .send(RouterMessage::Track {
                from: self.address.clone(),
                to: address,
            })
            .await?;
        Ok(())
    }

    /// Tell one address to track this agents address.
    /// If this agents address is unregistered, the tracking agent will
    /// receive a `Message::AgentRemoved(tracked_address)`.
    pub async fn reverse_track(&self, address: A) -> Result<()> {
        self.router_tx
            .send(RouterMessage::Track {
                from: address,
                to: self.address.clone(),
            })
            .await?;
        Ok(())
    }

    /// The agents address
    pub fn address(&self) -> &A {
        &self.address
    }

    pub async fn recv(&mut self) -> Result<Message<T, A>> {
        let msg =
            self.rx.recv_async().await.map_err(|_| Error::ChannelClosed)?;
        msg.into_local_message()
    }

    pub fn recv_sync(&mut self) -> Result<Message<T, A>> {
        let msg = self.rx.recv().map_err(|_| Error::ChannelClosed)?;
        msg.into_local_message()
    }

    pub async fn send<U: Send + 'static>(
        &self,
        recipient: A,
        message: U,
    ) -> Result<()> {
        let router_msg = RouterMessage::Message {
            recipient,
            sender: self.address.clone(),
            msg: AnyMessage::new(message),
        };
        self.router_tx.send(router_msg).await?;
        Ok(())
    }

    pub async fn send_remote(
        &self,
        recipients: impl IntoIterator<Item = A>,
        message: &[u8],
    ) -> Result<()> {
        let framed_message = Frame::frame_message(message);

        for recipient in recipients.into_iter() {
            let router_msg = RouterMessage::Message {
                recipient,
                sender: self.address.clone(),
                msg: AnyMessage::new(framed_message.clone()),
            };
            self.router_tx.send(router_msg).await?;
        }

        Ok(())
    }

    /// Tell the router to shut down an agent
    pub async fn send_shutdown(&self, recipient: A) -> Result<()> {
        self.router_tx.send(RouterMessage::Shutdown(recipient)).await
    }

    /// This is used for debugging, to print
    /// the current list of registered channels on a router.
    pub fn print_channels(&self) {
        let _ = self.router_tx.send(RouterMessage::PrintChannels);
    }

    /// Shutdown the agent and unregister it with the router.
    pub fn shutdown(&self) {
        let router_msg = RouterMessage::Shutdown(self.address.clone());
        let _ = self.router_tx.send(router_msg);
    }

    /// Shutdown the router and unregister ALL agents with the router.
    /// Any agent registered with the router should be dropped at this point
    /// as they can no longer receive or send message.
    pub async fn shutdown_router(&self) {
        let _ = self.router_tx.send(RouterMessage::ShutdownRouter).await;
    }
}

impl<T: Send + 'static, A: ToAddress + Into<Option<Vec<u8>>>> Agent<T, A> {
    pub async fn send_bridged(
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
        self.router_tx.send(router_msg).await?;
        Ok(())
    }
}
