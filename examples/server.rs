use std::fs::remove_file;

use tinyroute::server::{Server, UdsListener, TcpListener};
use tinyroute::{Agent, Message, Router, ToAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Address {
    Server,
    Uds,
    Tcp,
    Log,
    TcpCon(usize),
    UdsCon(usize),
}

impl ToAddress for Address {
    fn from_bytes(bytes: &[u8]) -> Option<Address> {
        match bytes {
            b"log" => Some(Address::Log),
            _ => None
        }
    }

    fn to_string(&self) -> String {
        format!("{:?}", self)
    }
}


async fn log(mut agent: Agent<(), Address>) {
    while let Ok(Message::RemoteMessage { sender, host, bytes }) = agent.recv().await {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            println!("{}@{} > {}", sender.to_string(), host, s);
        }
    }
}

#[tokio::main]
async fn main() {
    // Clean up possible stale socket
    let socket_path = "/tmp/example-server.sock";
    let _ = remove_file(socket_path);

    let mut router = Router::<Address>::new();

    let log_agent = router.new_agent(1024, Address::Log).unwrap();
    let uds_agent = router.new_agent(1024, Address::Uds).unwrap();
    let tcp_agent = router.new_agent(1024, Address::Tcp).unwrap();

    let uds_listener = UdsListener::bind(socket_path).unwrap();
    let tcp_listener = TcpListener::bind("127.0.0.1:6789").await.unwrap();
    let uds_server = Server::new(uds_listener, uds_agent);
    let tcp_server = Server::new(tcp_listener, tcp_agent);

    // Start the Uds server
    let router_tx = router.router_tx();
    tokio::spawn(async move { 
        let mut id = 0;
        uds_server.run(router_tx, None, || {
            id += 1;
            Address::UdsCon(id)
        }).await.unwrap(); 
    });

    // Start the Tcp server
    let router_tx = router.router_tx();
    tokio::spawn(async move {
        let mut id = 0;
        tcp_server.run(router_tx, None, || {
            id += 1;
            Address::TcpCon(id)
        }).await.unwrap(); 
    });

    tokio::spawn(router.run());

    // Block on the log
    log(log_agent).await;
}
