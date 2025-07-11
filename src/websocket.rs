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

async fn start_websocket(mut shutdown: broadcast::Receiver<()>) {
    // Begin websocket on specified IP and Port
    let listener = TcpListener::bind(WEBSOCKET_IP_AND_PORT.clone())
        .await
        .unwrap();

    println!("Web socket listening on ws://{}", *WEBSOCKET_IP_AND_PORT);

    // Keep track of ongoing connections
    let clients = Arc::new(Mutex::new(vec![]));
    let client_names = Arc::new(Mutex::new(vec![]));

    let messages = Arc::new(Mutex::new(vec![]));

    loop {
        tokio::select! {
            // Listen for new connections, then handle that connection
            Ok((stream, _)) = listener.accept() => {
                tokio::spawn(handle_connection(stream, clients.clone(), client_names.clone(), messages.clone()));
            },
            // If shutdown receives a message, stop the socket from running
            _ = shutdown.recv() => {
                println!("Websocket server shutting down");
                break;
            }
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    clients: Arc<Mutex<Vec<(usize, Tx)>>>,
    client_names: Arc<Mutex<Vec<(usize, String)>>>,
    messages: Arc<Mutex<Vec<(usize, Message)>>>,
) {
    // Get handles for the websocket, sending and receiving
    let ws_stream = accept_async(stream).await.unwrap();
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Create channels for communication to this client
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    // Generate a name which isn't already taken
    let client_name = {
        // Get a list of the current client_names
        let mut name_list = {
            let names_guard = client_names.lock().unwrap();

            names_guard.clone().into_iter().map(|(_, name)| name)
        };

        // Generate a name for the client
        let mut client_name = generate_name();

        // Keep generating until the name is unique
        while name_list.any(|name| name == client_name) {
            client_name = generate_name();
        }

        client_name
    };

    // Send new client the chat history
    {
        let messages_guard = messages.lock().unwrap();
        for (from_id, message) in messages_guard.iter() {
            let m = format_message_to_client(
                *from_id,
                message.clone().to_string(),
                client_names.clone(),
            )
            .unwrap();
            tx.send(m).unwrap();
        }
    }

    // Add the client's sender to clients Vec
    let client_index = {
        let mut clients_guard = clients.lock().unwrap();

        // Get the index of this client, choosing the lowest available ID
        let mut client_index = clients_guard.len();
        for (i, &client_idx) in clients_guard
            .iter()
            .map(|(client_idx, _)| client_idx)
            .enumerate()
        {
            // Client ID not the same as what it should be, means that the index it should be is free
            if client_idx != i {
                client_index = i
            }
        }
        println!("New web socket connection [{client_index} {client_name}]");

        // Add client's mpsc sender to client list
        clients_guard.push((client_index, tx.clone()));

        // Add the client name to client_names
        {
            let mut names_guard = client_names.lock().unwrap();

            names_guard.push((client_index, client_name.clone()));
        }

        // Broadcast that the client has joined
        let m = format_message_to_client(
            client_index,
            "Joined the chat".to_string(),
            client_names.clone(),
        )
        .unwrap();
        for client in clients_guard.iter() {
            if client.0 != client_index {
                let _ = client.1.send(m.clone());
            }
        }

        // Add message to messages Vec
        {
            let mut messages_guard = messages.lock().unwrap();
            messages_guard.push((client_index, Message::from("Joined the chat"))); // TODO duplicated code
        }

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
            println!("Recieved Message [{client_index} {client_name}]: {message:?}");

            // Message to broadcast
            let m = format_message_to_client(
                client_index,
                message.clone().to_string(),
                client_names.clone(),
            )
            .unwrap();

            // Broadcast message to all clients
            {
                let clients_guard = clients.lock().unwrap();
                for client in clients_guard.iter() {
                    // Don't broadcast the message to your own client
                    if client.0 != client_index {
                        // let m = Message::Text(format!("[{client_name}] {message}").into());
                        client.1.send(m.clone()).unwrap();
                    }
                }
            }

            // Add message to messages Vec
            {
                let mut messages_guard = messages.lock().unwrap();
                messages_guard.push((client_index, message));
            }
        }
    }

    println!("Web socket connection [{client_index} {client_name}] closed");

    // Stop sending to client since connection has ended
    send_task.abort();

    // Message to broadcast client leaving
    let m = format_message_to_client(
        client_index,
        "Left the chat".to_string(),
        client_names.clone(),
    )
    .unwrap();

    // Remove the tx from the clients list
    {
        let mut clients_guard = clients.lock().unwrap();

        clients_guard.remove(client_index);

        // Remove the client name at the same index
        {
            let mut names_guard = client_names.lock().unwrap();

            names_guard.remove(client_index);
        }

        // Broadcast that the client has left
        for client in clients_guard.iter() {
            let _ = client.1.send(m.clone());
        }
    }

    // Add message to messages Vec
    {
        let mut messages_guard = messages.lock().unwrap();
        messages_guard.push((client_index, m));
    }
}

fn format_message_to_client(
    from_id: usize,
    message: String,
    client_names: Arc<Mutex<Vec<(usize, String)>>>,
) -> Option<Message> {
    // Get the name from the ID
    let from_name = {
        let names_guard = client_names.lock().unwrap();
        names_guard.iter().find_map(|(i, name)| {
            if *i == from_id {
                Some(name.clone())
            } else {
                None
            }
        })
    };

    let from_name = if let Some(from_name) = from_name {
        from_name
    } else {
        format!("{{{from_id}}}")
    };

    // Format message
    Some(Message::Text(format!("[{from_name}] {message}").into()))
}

const NAME_ADJECTIVES: &[&str] = &[
    "happy", "brave", "calm", "gentle", "bright", "quick", "slow", "wise", "strong", "kind",
    "fierce", "quiet", "loud", "warm", "cold", "sharp", "soft", "hard", "light", "dark", "fresh",
    "stale", "young", "old", "new", "ancient", "sweet", "bitter", "salty", "spicy", "large",
    "small", "tiny", "huge", "narrow", "wide", "tall", "short", "deep", "shallow", "clean",
    "dirty", "rich", "poor", "fancy", "plain", "curious", "serious", "funny", "sad",
];

const NAME_NOUNS: &[&str] = &[
    "dog", "cat", "tree", "mountain", "river", "ocean", "star", "sun", "moon", "cloud", "bird",
    "flower", "stone", "house", "car", "book", "chair", "table", "door", "window", "road", "city",
    "village", "forest", "field", "garden", "school", "teacher", "student", "friend", "child",
    "parent", "king", "queen", "soldier", "artist", "doctor", "farmer", "worker", "leader", "song",
    "dream", "story", "game", "idea", "plan", "truth", "hope", "love", "fear",
];

const NAME_ADVERBS: &[&str] = &[
    "quickly",
    "slowly",
    "quietly",
    "loudly",
    "happily",
    "sadly",
    "angrily",
    "eagerly",
    "gently",
    "bravely",
    "carefully",
    "recklessly",
    "calmly",
    "boldly",
    "sharply",
    "softly",
    "warmly",
    "coldly",
    "brightly",
    "darkly",
    "gracefully",
    "clumsily",
    "easily",
    "hardly",
    "barely",
    "truly",
    "honestly",
    "secretly",
    "openly",
    "proudly",
    "politely",
    "rudely",
    "kindly",
    "cruelly",
    "freely",
    "tightly",
    "loosely",
    "firmly",
    "weakly",
    "strongly",
    "cheerfully",
    "seriously",
    "curiously",
    "suddenly",
    "eventually",
    "immediately",
    "rarely",
    "often",
    "always",
    "never",
];

const NAME_VERBS: &[&str] = &[
    "run", "walk", "jump", "fly", "swim", "climb", "sit", "stand", "sleep", "wake", "eat", "drink",
    "cook", "write", "read", "draw", "build", "break", "open", "close", "find", "lose", "give",
    "take", "send", "receive", "buy", "sell", "teach", "learn", "sing", "dance", "play", "work",
    "rest", "think", "dream", "hope", "love", "hate", "start", "stop", "begin", "end", "win",
    "fail", "help", "watch", "listen", "talk",
];

fn generate_name() -> String {
    use rand::seq::IteratorRandom;

    let mut rng = rand::rng();

    let (word_one, word_two);
    // Choose from nouns or verbs
    if rand::random::<bool>() {
        // Nouns
        word_one = NAME_ADJECTIVES.iter().choose(&mut rng).unwrap();
        word_two = NAME_NOUNS.iter().choose(&mut rng).unwrap();
    } else {
        // Verbs
        word_one = NAME_ADVERBS.iter().choose(&mut rng).unwrap();
        word_two = NAME_VERBS.iter().choose(&mut rng).unwrap();
    }

    format!("{word_one}-{word_two}")
}
