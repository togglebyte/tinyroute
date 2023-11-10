use crate::agent::AgentMsg;
use crate::ToAddress;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
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

    #[cfg(feature = "tls")]
    #[error(transparent)]
    Tls(#[from] TlsError),
}

impl<A: ToAddress> From<flume::SendError<AgentMsg<A>>> for Error {
    fn from(_: flume::SendError<AgentMsg<A>>) -> Self {
        Self::ChannelClosed
    }
}

#[cfg(feature = "tls")]
#[derive(thiserror::Error, Debug)]
pub enum TlsError {
    #[error("The provided domain name appears to be invalid")]
    InvalidDnsName(#[from] tokio_rustls::rustls::client::InvalidDnsNameError),
    #[error("Failed due to a TLS related issue")]
    Rustls(#[from] tokio_rustls::rustls::Error),
}
