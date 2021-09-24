use tinyroute::{Agent, Message, Router, ToAddress};
use tinyroute::server::{Server, TcpListener, UdsListener};
use tinyroute::client::{UdsClient, connect);
use tinyroute::errors::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Address {
    A,
    Server,
    Con,
}

impl ToAddress for Address {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match bytes {
            _ => None,
        }
    }
}

fn setup() -> (Agent<String, Address>, Agent<(), Address>, Router<Address>) {
    let mut router = Router::new();
    let agent_a = router.new_agent(10, Address::A).unwrap();
    let agent_b = router.new_agent(10, Address::Server).unwrap();

    (agent_a, agent_b, router)
}

#[tokio::test]
async fn remote_message() {
    let (agent_a, server_agent, router) = setup();
    let router_tx = router.router_tx();
    let handle = tokio::spawn(async move { router.run().await });

    let path = "/tmp/tinyroute-server-test.sock";
    let _ = std::fs::remove_file(path);
    let listener = UdsListener::bind(path).unwrap();

    let server = Server::new(listener, server_agent);
    let connection = server.next(router_tx, Addres::Con, None, 10).await.unwrap();

    thread::spawn(|| {
        let uds_client = UdsClient::connect(path);

    });


    agent_a.shutdown_router();
    handle.await;
}
