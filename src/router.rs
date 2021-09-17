use std::collections::HashMap;

use bytes::Bytes;
use log::{error, info};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::agent::{
    Agent, AgentMsg, AnyMessage, Deserializer, Local, Remote, Serializer,
};
use crate::errors::{Error, Result};

// -----------------------------------------------------------------------------
//     - Router TX -
// -----------------------------------------------------------------------------
#[derive(Clone)]
pub struct RouterTx<A: ToAddress>(
    pub(crate) mpsc::UnboundedSender<RouterMessage<A>>,
);

impl<A: ToAddress> RouterTx<A> {
    pub(crate) fn register_agent(
        &self,
        address: A,
        tx: mpsc::Sender<AgentMsg<A>>,
        channel: Channel,
    ) -> Result<()> {
        match self.0.send(RouterMessage::Register(address, tx, channel)) {
            Ok(()) => Ok(()),
            Err(_) => {
                error!("Failed to register local agent");
                Err(Error::RegisterAgentFailed)
            }
        }
    }

    pub(crate) fn send(&self, msg: RouterMessage<A>) -> Result<()> {
        match self.0.send(msg) {
            Ok(()) => Ok(()),
            Err(_e) => Err(Error::RouterMessage),
        }
    }
}

pub trait ToAddress: Send + Clone + Eq + std::hash::Hash + 'static {
    fn from_bytes(bytes: &[u8]) -> Option<Self>;

    fn to_string(&self) -> String {
        "[not implemented for this address]".into()
    }
}

// -----------------------------------------------------------------------------
//     - Router -
// -----------------------------------------------------------------------------
pub(crate) enum RouterMessage<A: ToAddress> {
    Message { recipient: A, sender: A, msg: AnyMessage },
    // The only thing that should be sending these remote messages
    // are the reader halves of a socket!
    RemoteMessage { recipient: A, sender: A, bytes: Bytes },
    Register(A, mpsc::Sender<AgentMsg<A>>, Channel),
    Track { from: A, to: A },
    Unregister(A),
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum Channel {
    Local,
    Remote,
}

// -----------------------------------------------------------------------------
//     - Router -
// -----------------------------------------------------------------------------
pub struct Router<A: ToAddress> {
    rx: mpsc::UnboundedReceiver<RouterMessage<A>>,
    tx: mpsc::UnboundedSender<RouterMessage<A>>,
    channels: HashMap<A, (mpsc::Sender<AgentMsg<A>>, Channel)>,
    subscriptions: HashMap<A, Vec<A>>,
}

impl<A: ToAddress + Clone> Router<A> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx, channels: HashMap::new(), subscriptions: HashMap::new() }
    }

    pub fn new_local_agent<T: Send + 'static>(
        &mut self,
        cap: usize,
        address: A,
    ) -> Option<Agent<Local<T, A>, A>> {
        let (tx, transport_rx) = mpsc::channel(cap);
        let transport = Local::new(transport_rx);
        let agent = Agent::new(self.router_tx(), transport, address.clone());
        if self.channels.contains_key(&address) {
            return None;
        }
        self.channels.insert(address, (tx, Channel::Local));
        Some(agent)
    }

    pub fn new_remote_agent<T: Send + 'static, Ser, De>(
        &mut self,
        cap: usize,
        address: A,
        serializer: Ser,
        deserializer: De,
    ) -> Option<Agent<Remote<T, A, Ser, De>, A>>
    where
        T: Send + DeserializeOwned + Serialize + 'static,
        Ser: Serializer<T>,
        De: Deserializer<T>,
        A: ToAddress,
    {
        let (tx, transport_rx) = mpsc::channel(cap);
        let transport = Remote::new(transport_rx, serializer, deserializer);
        let agent = Agent::new(self.router_tx(), transport, address.clone());
        if self.channels.contains_key(&address) {
            return None;
        }
        self.channels.insert(address, (tx, Channel::Remote));
        Some(agent)
    }

    pub fn router_tx(&self) -> RouterTx<A> {
        RouterTx(self.tx.clone())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                RouterMessage::Message { sender, recipient, msg } => {
                    let (tx, _) = match self.channels.get(&recipient) {
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
                RouterMessage::RemoteMessage { recipient, sender, bytes } => {
                    let tx = match self.channels.get(&recipient) {
                        Some((_, Channel::Local)) => {
                            error!("\"{}\" tried to send a remote message to a local channel: \"{}\"", sender.to_string(), recipient.to_string());
                            continue;
                        }
                        Some((tx, _)) => tx,
                        None => {
                            info!(
                                "No channel registered at \"{}\"",
                                recipient.to_string()
                            );
                            continue;
                        }
                    };

                    if tx
                        .send(AgentMsg::RemoteMessage(bytes, sender))
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
                RouterMessage::Register(address, tx, channel) => {
                    if self.channels.contains_key(&address) { continue }
                    let address_str = address.to_string();
                    self.channels.insert(address, (tx, channel));
                    info!(
                        "Registered \"{}\" as a {:?}",
                        address_str,
                        channel
                    );
                }
                RouterMessage::Track { from, to } => {
                    let tracked =
                        self.subscriptions.entry(to).or_insert(Vec::new());

                    if tracked.contains(&from) {
                        continue;
                    }

                    tracked.push(from);
                }
                RouterMessage::Unregister(address) => {
                    if self.channels.remove(&address).is_none() {
                        continue;
                    }

                    let subs = match self.subscriptions.remove(&address) {
                        None => continue,
                        Some(s) => s,
                    };

                    for s in subs {
                        self.subscriptions.get_mut(&s).map(|relations| {
                            relations.retain(|p| p != &address);
                        });
                        let address = address.clone();
                        if let Some((tx, _)) = self.channels.get(&s) {
                            let _ = tx.send(AgentMsg::AgentRemoved(address)).await;
                        }
                    }
                }
            }
        }
    }
}
