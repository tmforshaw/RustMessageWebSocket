// #[macro_use]
extern crate rocket;

use rocket::fs::{FileServer, relative};
use tokio::task;

use crate::websocket::start_websocket;

mod websocket;

async fn launch_rocket() {
    rocket::build()
        // .mount("/", routes![index])
        .mount("/", FileServer::from(relative!("static")))
        .launch()
        .await
        .unwrap();
}

// TODO add mpsc so i can allow communication between clients

#[tokio::main]
async fn main() {
    let rocket_handle = task::spawn(launch_rocket());

    let websocket_handle = task::spawn(start_websocket());

    let _ = tokio::join!(rocket_handle, websocket_handle);
}
