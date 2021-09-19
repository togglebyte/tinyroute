use std::time::Duration;

use log::{error, info};
use crate::agent::{Agent, Message};
use crate::client::{
    connect, Client, ClientMessage, ClientReceiver, ClientSender, TcpClient,
};
use crate::errors::Result;
use crate::frame::Frame;
use crate::ToAddress;

#[derive(Debug, Clone)]
pub enum Reconnect {
    Constant(Duration),
    Exponential(u64, Option<usize>),
}

pub enum Retry {
    Never,
    Forever,
    Count(usize),
}

async fn connect_to(
    addr: impl AsRef<str>,
    mut reconnect: Reconnect,
    heartbeat: Option<Duration>,
) -> (ClientSender, ClientReceiver) {
    let tcp_client = loop {
        match TcpClient::connect(addr.as_ref()).await {
            Ok(c) => break c,
            Err(e) => {
                let sleep_time = match reconnect {
                    Reconnect::Constant(n) => n,
                    Reconnect::Exponential(mut n, max) => {
                        let sleep = Duration::from_secs(n);
                        n *= 2;
                        reconnect = Reconnect::Exponential(n, max);
                        sleep
                    }
                };
                tokio::time::sleep(sleep_time).await;
                info!("retrying...");
                continue;
            }
        }
    };

    connect(tcp_client, heartbeat)
}

// This is a bit silly but I'm pretty tired
enum Action<T> {
    Message(T),
    Reconnect,
    Continue,
}

impl<T> std::fmt::Display for Action<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(_) => write!(f, "Message"),
            Self::Reconnect => write!(f, "Reconnect"),
            Self::Continue => write!(f, "Continue"),
        }
    }
}

pub async fn bridge<T: Send + 'static, A: ToAddress>(
    mut agent: Agent<T, A>,
    addr: impl AsRef<str> + Copy,
    mut reconnect: Reconnect,
    heartbeat: Option<Duration>,
) {
    let mut queue = std::collections::VecDeque::<()>::new();

    // Rx here is the incoming data from the network connection.
    // This should never do anything but return `None` once the connection is closed.
    // No data should be sent to this guy
    let (mut bridge_output_tx, mut rx_client_closed) =
        connect_to(addr, reconnect.clone(), heartbeat.clone()).await;
    eprintln!("{:?}", "connected");

    loop {
        let action = tokio::select! {
            is_closed = rx_client_closed.recv() => {
                let is_closed = is_closed.is_none();
                match is_closed {
                    true => Action::Reconnect,
                    false => Action::Continue,
                }
            },
            msg = agent.recv() => {
                match msg {
                    Ok(m) => Action::Message(m),
                    Err(e) => {
                        error!("failed to receive message: {:?}", e);
                        Action::Continue
                    }
                }
            }
        };

        eprintln!("--- Action> {}", action);

        let msg = match action {
            Action::Message(m) => {
                eprintln!("> Message");
                m
            }
            Action::Reconnect => {
                eprintln!("> Reconnect...");
                let (new_tx, mut new_rx) =
                    connect_to(addr, reconnect.clone(), heartbeat.clone())
                        .await;
                // Everything in the output here should be kept,
                // but we can't do that so we need a new way of dealing with this
                bridge_output_tx = new_tx;
                rx_client_closed = new_rx;
                eprintln!("> Connected");
                continue;
            }
            Action::Continue => {
                eprintln!("> Continue");
                continue;
            }
        };

        eprintln!("Trying to send the message...");

        match msg {
            // Only send remote messages
            Message::RemoteMessage(bytes, sender) => {
                eprintln!("{:?}", std::str::from_utf8(&bytes).unwrap());

                // Framed messages only!
                let framed_message = Frame::frame_message(&bytes);
                let msg = ClientMessage::Payload(framed_message);
                let res = bridge_output_tx.send(msg).await;
                eprintln!("Mesage sent: > {:?}", res.is_ok());
            }
            Message::Value(val, sender) => {}
            Message::Shutdown => {
                eprintln!("{:?}", "this is great!");
            }
            Message::AgentRemoved(address) => {}
        }
    }
}
