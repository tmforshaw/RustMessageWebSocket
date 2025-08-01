// Connect to websocket
const ws = new WebSocket("ws://127.0.0.1:4321");

// Define functions to run on websocket events
ws.onopen = () => add_message("WebSocket connected");
ws.onmessage = (e) => add_message(receieved_to_display_message(e.data));
ws.onclose = () => add_message("WebSocket disconnected");

// Send a message to the socket, adding the message to messages div
function send(message) {
  if (message.length > 0) {
    ws.send(message);
    add_message("Sent: " + message);
  }
}

// Function to add a message to the messages div
function add_message(msg) {
  let messages_div = document.getElementById("messages");

  // Create new message element
  let message_element = document.createElement("div");
  message_element.className = "message";
  message_element.innerText = msg;

  // Add message element to the messages div
  messages_div.appendChild(message_element);
}

function receieved_to_display_message(received_message) {
  // The sender's ID is within square brackets
  let match = received_message.match(/\[[^\]]\]/);

  // Should not fail this match
  if (match) {
    // Split the received message to retrieve the sender ID
    let sender_id = match[0];
    let message = received_message.slice(match.index + match[0].length);

    // Format the message, including the sender ID
    return `${sender_id}: ${message}`;
  }

  return received_message;
}

// When Enter is pressed within the input, clear it and send the value to the websocket
let input = document.getElementById("input");
input.addEventListener(
  "keydown", (ev) => {
    if (ev.key === "Enter") {
      // Get data from input box, then clear it
      let data = input.value;
      input.value = "";

      // Send data to websocket
      send(data);
    }
});
