use std::collections::HashMap;

use bytes::Bytes;
use log::{error, info, warn};
use flume::bounded;

#[cfg(feature="tokio_rt")]
#[cfg(not(feature="async_std_rt"))]
use tokio::spawn;

#[cfg(feature="async_std_rt")]
#[cfg(not(feature="tokio_rt"))]
use async_std::task::spawn;

use crate::agent::{Agent, AgentMsg, AnyMessage};
use crate::errors::{Error, Result};
use crate::server::ConnectionAddr;

// -----------------------------------------------------------------------------
//     - Router TX -
// -----------------------------------------------------------------------------
#[derive(Clone)]
pub struct RouterTx<A: ToAddress>(pub(crate) flume::Sender<RouterMessage<A>>);

impl<A: ToAddress> RouterTx<A> {
    pub(crate) async fn register_agent(&self, address: A, tx: flume::Sender<AgentMsg<A>>) -> Result<()> {
        let (success_tx, success_rx) = bounded(0);
        self.0.send(RouterMessage::Register(address, tx, success_tx)).map_err(|_| Error::RegisterAgentFailed)?;
        success_rx.recv_async().await.map_err(|_| Error::RegisterAgentFailed)?;
        Ok(())
    }

    pub(crate) async fn send(&self, msg: RouterMessage<A>) -> Result<()> {
        match self.0.send_async(msg).await {
            Ok(()) => Ok(()),
            Err(_) => Err(Error::RouterUnrecoverableError),
        }
    }

    pub(crate) fn send_sync(&self, msg: RouterMessage<A>) -> Result<()> {
        match self.0.send(msg) {
            Ok(()) => Ok(()),
            Err(_) => Err(Error::RouterUnrecoverableError),
        }
    }
}

/// Convert bytes into an address, get a string representation of an address.
pub trait ToAddress: Send + Clone + Eq + std::hash::Hash + 'static {
    fn from_bytes(bytes: &[u8]) -> Option<Self>;

    fn to_string(&self) -> String {
        "[not implemented for this address]".into()
    }
}

pub trait AddressToBytes {
    fn to_bytes(&self) -> Vec<u8>;
}

// -----------------------------------------------------------------------------
//     - Router -
// -----------------------------------------------------------------------------
pub(crate) enum RouterMessage<A: ToAddress> {
    Message { recipient: A, sender: A, msg: AnyMessage },
    // The only thing that should be sending these remote messages
    // are the reader halves of a socket!
    RemoteMessage { recipient: A, sender: A, bytes: Bytes, host: ConnectionAddr },
    Register(A, flume::Sender<AgentMsg<A>>, flume::Sender<()>),
    Track { from: A, to: A },
    Unregister(A),
    Shutdown(A),
    PrintChannels,
    ShutdownRouter,
}

// -----------------------------------------------------------------------------
//     - Router -
// -----------------------------------------------------------------------------
/// The `Router` is in charge of routing messages
/// between agents.
///
/// ```
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # pub enum Address {
/// #     A,
/// #     B,
/// # }
/// # 
/// # impl ToAddress for Address {
/// #     fn from_bytes(bytes: &[u8]) -> Option<Address> {
/// #         match bytes {
/// #             _ => None
/// #         }
/// #     }
/// # 
/// #     fn to_string(&self) -> String {
/// #         format!("{:?}", self)
/// #     }
/// # }
/// use tinyroute::{ToAddress, Router, Message};
/// # async fn run() {
///
/// let mut router = Router::<Address>::new();
/// let mut agent_a = router.new_agent::<()>(1, Address::A).unwrap();
/// let mut agent_b = router.new_agent::<()>(1, Address::B).unwrap();
///
/// agent_a.send(Address::B, ());
///
/// let val = agent_b.recv().await;
/// # }
/// ```
pub struct Router<A: ToAddress> {
    rx: flume::Receiver<RouterMessage<A>>,
    tx: flume::Sender<RouterMessage<A>>,
    channels: HashMap<A, flume::Sender<AgentMsg<A>>>,
    subscriptions: HashMap<A, Vec<A>>,
}

impl<A: ToAddress + Clone> Router<A> {
    pub fn new() -> Self {
        let (tx, rx) = flume::unbounded();
        Self { tx, rx, channels: HashMap::new(), subscriptions: HashMap::new() }
    }

    pub fn new_agent<T: Send + 'static>(&mut self, cap: usize, address: A) -> Result<Agent<T, A>> {
        let (tx, transport_rx) = flume::bounded(cap);
        let agent = Agent::new(self.router_tx(), address.clone(), transport_rx);
        if self.channels.contains_key(&address) {
            warn!("There is already an agent registered at \"{}\"", address.to_string());
            return Err(Error::AddressRegistered);
        }
        self.channels.insert(address, tx);
        Ok(agent)
    }

    pub fn router_tx(&self) -> RouterTx<A> {
        RouterTx(self.tx.clone())
    }

    async fn unregister(&mut self, address: A) {
        if self.channels.remove(&address).is_none() {
            return;
        }

        let subs = match self.subscriptions.remove(&address) {
            None => return,
            Some(s) => s,
        };

        for s in subs {
            if let Some(relations) = self.subscriptions.get_mut(&s) {
                relations.retain(|p| p != &address);
            }

            let address = address.clone();
            if let Some(tx) = self.channels.get(&s) {
                let _ = tx.send_async(AgentMsg::AgentRemoved(address)).await;
            }
        }
    }

    pub async fn run(mut self) {
        while let Ok(msg) = self.rx.recv_async().await {
            match msg {
                RouterMessage::ShutdownRouter => {
                    let drain = self.channels.drain().map(|(_, tx)|tx);
                    for tx in drain {
                        spawn(async move {
                            let _ = tx.send_async(AgentMsg::Shutdown).await;
                        });
                    }

                    info!("Shutting down router");
                    break;
                }
                RouterMessage::PrintChannels => {
                    for k in self.channels.keys() {
                        println!("Chan: {}", k.to_string());
                    }
                }
                RouterMessage::Message { sender, recipient, msg } => {
                    let tx = match self.channels.get(&recipient) {
                        Some(val) => val,
                        None => {
                            info!("No channel registered at \"{}\"", recipient.to_string());
                            continue;
                        }
                    };

                    if tx.send_async(AgentMsg::Message(msg, sender)).await.is_err() {
                        error!("Failed to send a message to \"{}\"", recipient.to_string());
                        // The receiving half is closed on the agent so
                        // removeing the channel makes sense
                        self.channels.remove(&recipient);
                    }
                }
                RouterMessage::RemoteMessage { recipient, sender, bytes, host } => {
                    let tx = match self.channels.get(&recipient) {
                        Some(tx) => tx,
                        None => {
                            info!("No channel registered at \"{}\"", recipient.to_string());
                            continue;
                        }
                    };

                    if tx.send_async(AgentMsg::RemoteMessage(bytes, sender, host)).await.is_err() {
                        error!("Failed to send a message to \"{}\"", recipient.to_string());
                        // The receiving half is closed on the agent so
                        // removeing the channel makes sense
                        self.channels.remove(&recipient);
                    }
                }
                RouterMessage::Register(address, tx, success_tx) => {
                    if self.channels.contains_key(&address) {
                        warn!("There is already an agent registered at \"{}\"", address.to_string());
                        continue;
                    }
                    let address_str = address.to_string();
                    self.channels.insert(address, tx);
                    info!("Registered \"{}\"", address_str);
                    success_tx.send(()).unwrap();
                }
                RouterMessage::Track { from, to } => {
                    let tracked = self.subscriptions.entry(to).or_insert_with(Vec::new);

                    if tracked.contains(&from) {
                        continue;
                    }

                    tracked.push(from);
                }
                RouterMessage::Unregister(address) => self.unregister(address).await,
                RouterMessage::Shutdown(sender) => {
                    let tx = match self.channels.get(&sender) {
                        Some(val) => val,
                        None => {
                            info!("No channel registered at \"{}\"", sender.to_string());
                            continue;
                        }
                    };
                    let _ = tx.send_async(AgentMsg::Shutdown).await;
                    self.unregister(sender).await;
                }
            }
        }

        info!("Router shutdown successful");
    }
}

impl<A: ToAddress> Default for Router<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Address {
        Agent
    }
    
    impl ToAddress for Address {
        fn from_bytes(bytes: &[u8]) -> Option<Address> {
            match bytes {
                _ => None
            }
        }
    
        fn to_string(&self) -> String {
            format!("{:?}", self)
        }
    }

    #[test]
    fn agent_creation() {
        let mut router = Router::new();
        let agent = router.new_agent::<()>(1024, Address::Agent);
        let actual = agent.is_ok();
        let expected = true;
        assert_eq!(expected, actual);
    }
    
    #[test]
    fn failed_agent_creation() {
        // Fail to register an agent at an existing address
        let mut router = Router::new();
        let _agent_ok = router.new_agent::<()>(1024, Address::Agent);
        let agent_err = router.new_agent::<()>(1024, Address::Agent);
        let actual = agent_err.is_err();
        let expected = true;
        assert_eq!(expected, actual);
    }
}
