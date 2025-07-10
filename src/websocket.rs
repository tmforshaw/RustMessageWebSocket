use std::sync::LazyLock;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;

const WEBSOCKET_IP: &str = "127.0.0.1";
const WEBSOCKET_PORT: &str = "4321";

static WEBSOCKET_IP_AND_PORT: LazyLock<String> =
    LazyLock::new(|| format!("{WEBSOCKET_IP}:{WEBSOCKET_PORT}"));

pub async fn start_websocket() {
    let listener = TcpListener::bind(WEBSOCKET_IP_AND_PORT.clone())
        .await
        .unwrap();

    println!("Web socket listening on ws://{}", *WEBSOCKET_IP_AND_PORT);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream));
    }
}

async fn handle_connection(stream: TcpStream) {
    let websocket_stream = accept_async(stream).await.unwrap();
    let (mut write, mut read) = websocket_stream.split();

    println!("New web socket connection");

    while let Some(Ok(message)) = read.next().await {
        if message.is_text() || message.is_binary() {
            println!("Recieved Message: {message:?}");

            write.send(message).await.unwrap();
        }
    }

    println!("Web socket connction closed");
}
