// #[macro_use]
extern crate rocket;

use rocket::fs::{FileServer, relative};
use tokio::task;

use crate::websocket::{start_websocket, start_websocket_listen_shutdown};

mod websocket;

// Launch webpage using rocket socket, mounting the static folder as the root of the webpage
async fn launch_rocket() {
    rocket::build()
        .mount("/", FileServer::from(relative!("static")))
        .launch()
        .await
        .unwrap();
}

#[tokio::main]
async fn main() {
    // Spawn rocket and the websocket
    let rocket_handle = task::spawn(launch_rocket());
    let websocket_handle = task::spawn(start_websocket_listen_shutdown());

    // Wait on both handles finishing
    let _ = tokio::join!(rocket_handle, websocket_handle);
}
