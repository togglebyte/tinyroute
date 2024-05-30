use tinyroute::error::Error;
use tinyroute::{Agent, Message, Router, ToAddress};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Address {
    A,
    B,
}

impl ToAddress for Address {
    fn from_bytes(_bytes: &[u8]) -> Option<Self> {
        None
    }
}

fn setup() -> (
    Agent<String, Address>,
    Agent<String, Address>,
    tokio::task::JoinHandle<()>,
) {
    let mut router = Router::new();
    let agent_a = router.new_agent::<String>(Some(10), Address::A).unwrap();
    let agent_b = router.new_agent::<String>(Some(10), Address::B).unwrap();

    let handle = tokio::spawn(async move {
        router.run().await;
    });

    (agent_a, agent_b, handle)
}

#[tokio::test]
async fn agent_to_agent() {
    let (agent_a, mut agent_b, handle) = setup();

    let message = "hello world".to_string();
    agent_a.send(Address::B, message.clone()).await.unwrap();

    let msg = agent_b.recv().await.unwrap();
    assert!(matches!(msg, Message::Value(_message, Address::A)));

    agent_b.shutdown_router().await;
    handle.await.unwrap();
}

#[tokio::test]
async fn shutdown_router() {
    let (agent_a, _, handle) = setup();
    agent_a.shutdown_router().await;
    handle.await.unwrap();
}

#[tokio::test]
async fn shutdown_agent() {
    let (agent_a, mut agent_b, handle) = setup();
    agent_a.send_shutdown(Address::B).await.unwrap();
    let _ = agent_b.recv().await; // throw away the shutdown message
    let err = agent_b.recv().await;
    assert!(matches!(err, Err(Error::ChannelClosed)));
    agent_a.shutdown_router().await;
    handle.await.unwrap();
}
