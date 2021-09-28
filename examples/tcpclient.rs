use std::io::{stdin, Result};
use std::sync::mpsc;
use std::time::Duration;
use std::thread;
use std::env::args;

use tinyroute::client::{connect, TcpClient, ClientMessage};
use tinyroute::frame::{FramedMessage, Frame};

fn input() -> mpsc::Receiver<FramedMessage> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || -> Result<()> {
        let mut buffer = String::new();
        let stdin = stdin();

        loop {
            stdin.read_line(&mut buffer)?;
            let mut line = buffer.drain(..).collect::<String>();
            line.pop();
            let framed_msg = Frame::frame_message(line.as_bytes());
            let _ = tx.send(framed_msg);
        }
    });

    rx
}

async fn run(rx: mpsc::Receiver<FramedMessage>, port: u16) {
    let addr = format!("127.0.0.1:{}", port);
    let client = TcpClient::connect(addr).await.unwrap();
    let (write_tx, read_rx) = connect(client, Some(Duration::from_secs(30)));

    tokio::spawn(output(read_rx));

    while let Ok(bytes) = rx.recv() {
        if let Err(_) = write_tx.send(ClientMessage::Payload(bytes)).await {
            break
        }
    }
}

async fn output(mut read_rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Option<()> {
    loop {
        let payload = read_rx.recv().await?;
        let data = String::from_utf8(payload).ok()?;
        println!("data: {}", data);
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let port = args().skip(1).next().map(|s| s.parse::<u16>().ok()).flatten().unwrap_or(5000);
    let rx = input();
    run(rx, port).await;
}

