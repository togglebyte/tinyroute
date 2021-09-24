use std::collections::HashMap;

use bytes::Bytes;
use log::{error, info, warn};
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Agent, AgentMsg, AnyMessage};
use crate::errors::{Error, Result};

// -----------------------------------------------------------------------------
//     - Router TX -
// -----------------------------------------------------------------------------
#[derive(Clone)]
pub struct RouterTx<A: ToAddress>(
    pub(crate) mpsc::UnboundedSender<RouterMessage<A>>,
);

impl<A: ToAddress> RouterTx<A> {
    pub(crate) async fn register_agent(
        &self,
        address: A,
        tx: mpsc::Sender<AgentMsg<A>>,
    ) -> Result<()> {
        let (success_tx, success_rx) = oneshot::channel();
        self.0
            .send(RouterMessage::Register(address, tx, success_tx))
            .map_err(|_| Error::RegisterAgentFailed)?;
        success_rx.await.map_err(|_| Error::RegisterAgentFailed)?;
        Ok(())
    }

    pub(crate) fn send(&self, msg: RouterMessage<A>) -> Result<()> {
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
    RemoteMessage { recipient: A, sender: A, bytes: Bytes, host: String },
    Register(A, mpsc::Sender<AgentMsg<A>>, oneshot::Sender<()>),
    Track { from: A, to: A },
    Unregister(A),
    Shutdown(A),
    PrintChannels,
    ShutdownRouter,
}

// -----------------------------------------------------------------------------
//     - Router -
// -----------------------------------------------------------------------------
pub struct Router<A: ToAddress> {
    rx: mpsc::UnboundedReceiver<RouterMessage<A>>,
    tx: mpsc::UnboundedSender<RouterMessage<A>>,
    channels: HashMap<A, mpsc::Sender<AgentMsg<A>>>,
    subscriptions: HashMap<A, Vec<A>>,
}

impl<A: ToAddress + Clone> Router<A> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx, channels: HashMap::new(), subscriptions: HashMap::new() }
    }

    pub fn new_agent<T: Send + 'static>(
        &mut self,
        cap: usize,
        address: A,
    ) -> Result<Agent<T, A>> {
        let (tx, transport_rx) = mpsc::channel(cap);
        let agent = Agent::new(self.router_tx(), address.clone(), transport_rx);
        if self.channels.contains_key(&address) {
            warn!(
                "There is already an agent registered at \"{}\"",
                address.to_string()
            );
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
            self.subscriptions.get_mut(&s).map(|relations| {
                relations.retain(|p| p != &address);
            });
            let address = address.clone();
            if let Some(tx) = self.channels.get(&s) {
                let _ = tx.send(AgentMsg::AgentRemoved(address)).await;
            }
        }
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                RouterMessage::ShutdownRouter => {
                    let mut drain = self.channels.drain();
                    while let Some((_, tx)) = drain.next() {
                        tokio::spawn(async move { let _ = tx.send(AgentMsg::Shutdown).await; });
                    }

                    info!("Shutting down router");
                    break;
                }
                RouterMessage::PrintChannels => {
                    for (k, _) in &self.channels {
                        println!("Chan: {}", k.to_string());
                    }
                }
                RouterMessage::Message { sender, recipient, msg } => {
                    let tx = match self.channels.get(&recipient) {
                        Some(val) => val,
                        None => {
                            info!(
                                "No channel registered at \"{}\"",
                                recipient.to_string()
                            );
                            continue;
                        }
                    };

                    if let Err(_) =
                        tx.send(AgentMsg::Message(msg, sender)).await
                    {
                        error!(
                            "Failed to send a message to \"{}\"",
                            recipient.to_string()
                        );
                        // The receiving half is closed on the agent so
                        // removeing the channel makes sense
                        self.channels.remove(&recipient);
                    }
                }
                RouterMessage::RemoteMessage { recipient, sender, bytes, host } => {
                    let tx = match self.channels.get(&recipient) {
                        Some(tx) => tx,
                        None => {
                            info!(
                                "No channel registered at \"{}\"",
                                recipient.to_string()
                            );
                            continue;
                        }
                    };

                    if tx
                        .send(AgentMsg::RemoteMessage(bytes, sender, host))
                        .await
                        .is_err()
                    {
                        error!(
                            "Failed to send a message to \"{}\"",
                            recipient.to_string()
                        );
                        // The receiving half is closed on the agent so
                        // removeing the channel makes sense
                        self.channels.remove(&recipient);
                    }
                }
                RouterMessage::Register(address, tx, success_tx) => {
                    if self.channels.contains_key(&address) {
                        warn!(
                            "There is already an agent registered at \"{}\"",
                            address.to_string()
                        );
                        continue;
                    }
                    let address_str = address.to_string();
                    self.channels.insert(address, tx);
                    info!("Registered \"{}\"", address_str);
                    let _ = success_tx.send(());
                }
                RouterMessage::Track { from, to } => {
                    let tracked = self.subscriptions.entry(to).or_insert(Vec::new());

                    if tracked.contains(&from) {
                        continue;
                    }

                    tracked.push(from);
                }
                RouterMessage::Unregister(address) => {
                    self.unregister(address).await;
                }
                RouterMessage::Shutdown(sender) => {
                    let tx = match self.channels.get(&sender) {
                        Some(val) => val,
                        None => {
                            info!(
                                "No channel registered at \"{}\"",
                                sender.to_string()
                            );
                            continue;
                        }
                    };
                    let _ = tx.send(AgentMsg::Shutdown).await;
                    self.unregister(sender).await;
                }
            }
        }

        info!("Router shutdown successful");
    }
}
