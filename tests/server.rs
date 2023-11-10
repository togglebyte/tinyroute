use tinyroute::client::{connect, ClientMessage, UdsClient};
use tinyroute::server::{Server, UdsConnections};
use tinyroute::{Agent, Message, Router, ToAddress};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Address {
    A,
    Server,
    Con,
}

impl ToAddress for Address {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match bytes {
            b"con" => Some(Address::Con),
            _ => None,
        }
    }
}

fn setup() -> (Agent<String, Address>, Agent<(), Address>, Router<Address>) {
    let mut router = Router::new();
    let agent_a = router.new_agent(Some(10), Address::A).unwrap();
    let agent_b = router.new_agent(Some(10), Address::Server).unwrap();

    (agent_a, agent_b, router)
}

#[tokio::test]
async fn remote_message() {
    // Setup agents and router, and start the router
    let (agent_a, server_agent, router) = setup();
    let handle = tokio::spawn(async move { router.run().await });

    // Create a server using a unix socket
    let path = "/tmp/tinyroute-server-test.sock";
    let _ = std::fs::remove_file(path);
    let connections = UdsConnections::bind(path).await.unwrap();

    let mut server = Server::new(connections, server_agent);

    // Create a client in a separate task
    // and send a remote message
    tokio::spawn(async move {
        let uds_client = UdsClient::connect(path).await.unwrap();
        let (tx, _rx) = connect(uds_client, None);
        let message = ClientMessage::channel_payload(b"con", b"hello world");
        tx.send_async(message).await.unwrap();
    });

    let mut connection = server.next(Address::Con, None, None).await.unwrap();
    let msg = connection.recv().await.unwrap().unwrap();

    match msg {
        Message::RemoteMessage { bytes, .. } => assert_eq!(b"hello world", bytes.as_ref()),
        _ => panic!("invalid message"),
    }

    // Shutdown and cleanup
    agent_a.shutdown_router().await;
    handle.await.unwrap();
    let _ = std::fs::remove_file(path);
}
