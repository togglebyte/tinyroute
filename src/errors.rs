use tokio::sync::mpsc;
use crate::agent::AgentMsg;
use crate::ToAddress;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Io error")]
    Io(#[from] std::io::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Invalid message type sent to the Agent")]
    InvalidMessageType,

    #[error("Malformed header when framing message")]
    MalformedHeader,

    #[error("Failed to register agent")]
    RegisterAgentFailed,

    #[error("Failed to deliver router message")]
    RouterMessage,

    #[error("Failed to send message")]
    MessageSendFail,

    #[error("Remote message to local channel")]
    RemoteToLocal,

    #[error("Can not convert a remote message to a local message")]
    InvalidMessageConversion,

    #[error("Missing sender from the payload")]
    MissingSender,

    #[error("Address already registered")]
    AddressRegistered,
}

impl<A: ToAddress> From<mpsc::error::SendError<AgentMsg<A>>> for Error {
    fn from(_: mpsc::error::SendError<AgentMsg<A>>) -> Self {
        Self::MessageSendFail
    }
}
