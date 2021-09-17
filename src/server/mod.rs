use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use log::error;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time;

use crate::agent::{
    Agent, Message, Local,
};
use crate::errors::Result;
use crate::frame::{Frame, FramedMessage, FrameOutput};

mod tcp;

use crate::router::{Channel, RouterMessage, RouterTx, ToAddress};
pub use tcp::TcpServer;

#[cfg(target_os = "linux")]
mod uds;
#[cfg(target_os = "linux")]
pub use uds::UdsServer;

/// Client payload.
/// Access the bytes through `self.data()`
#[derive(Debug, Clone)]
pub(crate) struct Payload {
    inner: Vec<u8>,
    offset: usize,
}

impl Payload {
    pub(crate) fn new(offset: usize, inner: Vec<u8>) -> Self {
        Self { inner, offset }
    }

    /// Access the inner data of the payload.
    /// This is the data void of the address
    pub(crate) fn data(&self) -> &[u8] {
        &self.inner[self.offset..]
    }
}

/// Some kind of listener
pub trait Server: Sync {
    /// The reading half of the connection
    type Reader: AsyncRead + Unpin + Send + 'static;
    /// The writing half of the connection
    type Writer: AsyncWrite + Unpin + Send + 'static;

    // Accepts &self as arg
    // Returns a pinned boxed future, where
    // * any reference has to live for at least as long as &self,
    // * and it has to be valid to send this across thread boundaries
    //
    // We need the `Send` part because tokio::spawn might put this on another thread.
    // We need the life time because the thing we return can not hold a reference to
    // anything on &self that might be dropped before self.
    /// Accept incoming connections
    fn accept(&mut self) -> ServerFuture<'_, Self::Reader, Self::Writer>;
}

/// Because writing this entire trait malarkey is messy!
pub type ServerFuture<'a, T, U> =
    Pin<Box<dyn Future<Output = Result<(T, U)>> + Send + 'a>>;

pub struct TheServer<Srv: Server> {
    server: Srv,
}

impl<Srv: Server> TheServer<Srv> {
    pub fn new(server: Srv) -> Self {
        Self { server }
    }

    pub async fn next<A: Sync + ToAddress>(
        &mut self,
        router_tx: RouterTx<A>,
        address: A,
        timeout: Option<Duration>,
        cap: usize,
    ) -> Option<Connection<A, <Srv as Server>::Writer>> {
        let (reader, writer) = self.server.accept().await.ok()?;

        // Register the agent
        let (transport_tx, transport_rx) = mpsc::channel(cap);
        let transport = Local::new(transport_rx);
        router_tx.register_agent(address.clone(), transport_tx, Channel::Local).ok()?;
        let agent = Agent::new(router_tx.clone(), transport, address.clone());

        // Spawn the reader
        tokio::spawn(spawn_reader(reader, address, router_tx, timeout));

        Some(Connection::new(agent, writer))
    }
}

async fn spawn_reader<A, R>(
    mut reader: R,
    sender: A,
    router_tx: RouterTx<A>,
    timeout: Option<Duration>,
) where
    R: AsyncRead + Unpin,
    A: ToAddress,
{
    let mut frame = Frame::empty();
    loop {
        let read = async {
            let res = frame.async_read(&mut reader).await;

            match res {
                Ok(0) => false,
                Ok(_) => {
                    match frame.try_msg() {
                        Ok(Some(FrameOutput::Heartbeat)) => true,
                        Ok(Some(FrameOutput::Message(msg))) => {
                            let address = msg
                                .iter()
                                .cloned()
                                .take_while(|b| (*b as char) != '|')
                                .collect::<Vec<u8>>();

                            // return in the event of the index being
                            // larger than the payload it self
                            let index = address.len() + 1;
                            if index >= msg.len() {
                                return true;
                            }

                            let address = match A::from_bytes(&address) {
                                Some(a) => a,
                                None => return true,
                            };

                            let payload = Payload::new(index, msg);
                            let bytes = Bytes::from(payload.data().to_vec());

                            match router_tx.send(RouterMessage::RemoteMessage {
                                bytes,
                                sender: sender.clone(),
                                recipient: address,
                            }) {
                                Ok(_) => true,
                                Err(e) => false,
                            }
                        }
                        Ok(None) => true,
                        Err(e) => {
                            error!("invalid payload. {:?}", e);
                            false
                        }
                    }
                }
                Err(e) => {
                    error!("failed to reade from the socket. reason: {:?}", e);
                    false
                }
            }
        };

        let restart = match timeout {
            Some(timeout) => {
                tokio::select! {
                    _ = time::sleep(timeout) => true,
                    restart = read => { restart }
                }
            }
            None => read.await,
        };

        if !restart {
            break;
        }
    }
}

pub struct Connection<A, W>
where
    A: ToAddress,
    W: AsyncWrite,
{
    agent: Agent<Local<FramedMessage, A>, A>,
    writer: W,
}

impl<A, W> Connection<A, W>
where
    A: ToAddress,
    W: AsyncWrite + Unpin,
{
    pub fn new(agent: Agent<Local<FramedMessage, A>, A>, writer: W) -> Self {
        Self { agent, writer }
    }

    pub async fn recv(&mut self) -> Result<()> {
        if let Message::Value(framed_message, _)  = self.agent.recv().await? {
            self.writer.write_all(&framed_message.0).await;
        }
        Ok(())
    }
}
