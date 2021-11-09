use std::io::{stdin, Result};
use std::time::Duration;
use std::thread;
use std::env::args;

use tinyroute::client::{connect, UdsClient, ClientMessage};
use tinyroute::frame::{FramedMessage, Frame};
use tinyroute::spawn;

fn input() -> flume::Receiver<FramedMessage> {
    let (tx, rx) = flume::unbounded();

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

async fn run(rx: flume::Receiver<FramedMessage>, addr: String) {
    let client = UdsClient::connect(addr).await.unwrap();
    let (write_tx, read_rx) = connect(client, Some(Duration::from_secs(30)));

    let read_handle = spawn(output(read_rx));

    while let Ok(bytes) = rx.recv() {
        if let Err(_) = write_tx.send_async(ClientMessage::Payload(bytes)).await {
            break
        }
    }

    read_handle.await;
}

async fn output(read_rx: flume::Receiver<Vec<u8>>) -> Option<()> {
    loop {
        let payload = read_rx.recv_async().await.ok()?;
        let data = String::from_utf8(payload).ok()?;
        println!("data: {}", data);
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let addr = args().skip(1).next().expect("provide an address");
    let rx = input();
    run(rx, addr).await;
}

