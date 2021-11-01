use tokio::sync::mpsc;
use crate::agent::AgentMsg;
use crate::ToAddress;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Io error")]
    Io(#[from] std::io::Error),

    #[error("Receive error")]
    RecvErr(#[from] flume::RecvError),

    #[error("Try receive error")]
    TryRecvErr(#[from] flume::TryRecvError),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Invalid message type sent to the Agent")]
    InvalidMessageType,

    #[error("Malformed header when framing message")]
    MalformedHeader,

    #[error("Failed to register agent")]
    RegisterAgentFailed,

    #[error("Failed to deliver the message to the router")]
    RouterUnrecoverableError,

    #[error("Failed to send message to another channel")]
    GenericChannelSendError,

    #[error("Remote message to local channel")]
    RemoteToLocal,

    #[error("Can not convert a remote message to a local message")]
    InvalidMessageConversion,

    #[error("Missing sender from the payload")]
    MissingSender,

    #[error("Address already registered")]
    AddressRegistered,

    #[error("Bridgemalarkey")]
    Bridge(#[from] crate::bridge::BridgeError),
}

impl<A: ToAddress> From<mpsc::error::SendError<AgentMsg<A>>> for Error {
    fn from(_: mpsc::error::SendError<AgentMsg<A>>) -> Self {
        Self::GenericChannelSendError
    }
}
