use std::sync::{Arc, LazyLock, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    signal,
    sync::broadcast,
};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

const WEBSOCKET_IP: &str = "127.0.0.1";
const WEBSOCKET_PORT: &str = "4321";

static WEBSOCKET_IP_AND_PORT: LazyLock<String> =
    LazyLock::new(|| format!("{WEBSOCKET_IP}:{WEBSOCKET_PORT}"));

type Tx = tokio::sync::mpsc::UnboundedSender<Message>;

pub async fn start_websocket_listen_shutdown() {
    // Create a channel to notify websocket when to shutdown
    let (shutdown_tx, _) = broadcast::channel(1);

    // Start the websocket, providing the shutdown receiver
    let shutdown_socket = shutdown_tx.subscribe();
    tokio::spawn(start_websocket(shutdown_socket));

    // Wait for the Ctrl+C signal to shutdown
    signal::ctrl_c().await.unwrap();
    println!("Shutting websocket down");

    // Notify websocket to shutdown
    let _ = shutdown_tx.send(());

    // Give tasks time to shutdown
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}

pub async fn start_websocket(mut shutdown: broadcast::Receiver<()>) {
    // Begin websocket on specified IP and Port
    let listener = TcpListener::bind(WEBSOCKET_IP_AND_PORT.clone())
        .await
        .unwrap();

    println!("Web socket listening on ws://{}", *WEBSOCKET_IP_AND_PORT);

    // Keep track of ongoing connections
    let clients = Arc::new(Mutex::new(vec![]));

    loop {
        tokio::select! {
            // Listen for new connections, then handle that connection
            Ok((stream, _)) = listener.accept() => {
                tokio::spawn(handle_connection(stream, clients.clone()));
            },
            // If shutdown receives a message, stop the socket from running
            _ = shutdown.recv() => {
                println!("Websocket server shutting down");
                break;
            }
        }
    }
}

async fn handle_connection(stream: TcpStream, clients: Arc<Mutex<Vec<Tx>>>) {
    // Get handles for the websocket, sending and receiving
    let ws_stream = accept_async(stream).await.unwrap();
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Create channels for communication to this client
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    // Add the client's sender to clients Vec
    let client_index = {
        let mut clients_guard = clients.lock().unwrap();

        // Get the index of this client
        let client_index = clients_guard.len();
        println!("New web socket connection [{client_index}]");

        // Add client's mpsc sender to client list
        clients_guard.push(tx.clone());

        client_index
    };

    // Task used to send messages to the client
    let send_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if ws_write.send(message).await.is_err() {
                break;
            }
        }
    });

    // Receive messages from websocket and broadcast to all clients using mpsc
    while let Some(Ok(message)) = ws_read.next().await {
        if message.is_text() || message.is_binary() {
            println!("Recieved Message [{client_index}]: {message:?}");

            let clients_guard = clients.lock().unwrap();
            for (i, client) in clients_guard.iter().enumerate() {
                // Don't broadcast the message to your own client
                if i != client_index {
                    let m = Message::Text(format!("[{client_index}] {message}").into());
                    let _ = client.send(m);
                }
            }
        }
    }

    println!("Web socket connection [{client_index}] closed");

    // Stop sending to client since connection has ended
    send_task.abort();

    // Remove the tx from the clients list
    {
        let mut client_guard = clients.lock().unwrap();

        client_guard.remove(client_index);
    }
}
