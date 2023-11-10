use std::fs::{self, remove_file};
use std::io::Cursor;
use std::path::Path;

use log::debug;
use tinyroute::server::tls::TlsConnections;
use tinyroute::server::{Server, TcpConnections, TcpListener, UdsConnections};
use tinyroute::{Agent, Message, Router, ToAddress};
use tokio_rustls::rustls::{Certificate, PrivateKey};

const CERT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/localhost-cert.pem");
const KEY_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/localhost-key.pem");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Address {
    Server,
    Uds,
    Tcp,
    Tls,
    Log,
    TlsCon(usize),
    TcpCon(usize),
    UdsCon(usize),
}

impl ToAddress for Address {
    fn from_bytes(bytes: &[u8]) -> Option<Address> {
        match bytes {
            b"log" => Some(Address::Log),
            _ => None,
        }
    }

    fn to_string(&self) -> String {
        format!("{:?}", self)
    }
}

async fn log(mut agent: Agent<(), Address>) {
    while let Ok(Message::RemoteMessage {
        sender,
        host,
        bytes,
    }) = agent.recv().await
    {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            println!("{}@{} > {}", sender.to_string(), host, s);
        }
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // Clean up possible stale socket
    let socket_path = "/tmp/example-server.sock";
    let _ = remove_file(socket_path);

    let mut router = Router::<Address>::new();

    let log_agent = router.new_agent(Some(1024), Address::Log).unwrap();
    let uds_agent = router.new_agent(Some(1024), Address::Uds).unwrap();
    let tcp_agent = router.new_agent(Some(1024), Address::Tcp).unwrap();
    let tls_agent = router.new_agent(Some(1024), Address::Tls).unwrap();
    debug!("agents created");

    // Create some quick self-signed certificates for the sake of the example
    if !Path::new(CERT_PATH).exists() || !Path::new(KEY_PATH).exists() {
        let cert = rcgen::generate_simple_self_signed(["localhost".to_owned()]).unwrap();
        fs::write(CERT_PATH, cert.serialize_pem().unwrap()).unwrap();
        fs::write(KEY_PATH, cert.serialize_private_key_pem()).unwrap();
    }

    let uds_listener = UdsConnections::bind(socket_path).await.unwrap();
    let tcp_listener = TcpConnections::bind("127.0.0.1:6789").await.unwrap();
    let tls_listener = TlsConnections::new(
        TcpListener::bind("127.0.0.1:6790").await.unwrap(),
        vec![Certificate(
            rustls_pemfile::certs(&mut Cursor::new(fs::read(CERT_PATH).unwrap()))
                .unwrap()
                .remove(0),
        )],
        PrivateKey(
            rustls_pemfile::pkcs8_private_keys(&mut Cursor::new(fs::read(KEY_PATH).unwrap()))
                .unwrap()
                .remove(0),
        ),
    )
    .unwrap();
    debug!("listeners created");
    let uds_server = Server::new(uds_listener, uds_agent);
    let tcp_server = Server::new(tcp_listener, tcp_agent);
    let tls_server = Server::new(tls_listener, tls_agent);

    // Start the UDS server
    let uds_handle = tokio::spawn(async move {
        let mut id = 0;
        uds_server
            .run(None, None, || {
                id += 1;
                Address::UdsCon(id)
            })
            .await
            .unwrap();
    });

    // Start the TCP server
    let tcp_handle = tokio::spawn(async move {
        let mut id = 0;
        tcp_server
            .run(None, None, || {
                id += 1;
                Address::TcpCon(id)
            })
            .await
            .unwrap();
    });

    // Start the TLS server
    let tls_handle = tokio::spawn(async move {
        let mut id = 0;
        tls_server
            .run(None, None, || {
                id += 1;
                Address::TlsCon(id)
            })
            .await
            .unwrap();
    });

    debug!("running router");
    let router_handle = tokio::spawn(router.run());

    // Block on the log
    log(log_agent).await;
    let _ = router_handle.await;
    let _ = tls_handle.await;
    let _ = tcp_handle.await;
    let _ = uds_handle.await;
}
