#![allow(warnings)]

use std::{
    collections::HashMap,
    env,
    io::Error as IoError,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};

use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::Message;

type Tx = UnboundedSender<Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

use anyhow::Result;

use log::{debug, error, info};
use log::*;

async fn handle_connection(peer_map: PeerMap, raw_stream: TcpStream, addr: SocketAddr) -> Result<()> {

    let ws_stream = tokio_tungstenite::accept_async(raw_stream).await?;
    info!("Новое соединение по WebSocket: {}", addr);

    let (tx, rx) = unbounded();
    peer_map.lock().unwrap().insert(addr, tx);

    let (outgoing, incoming) = ws_stream.split();

    let broadcast_incoming = incoming.try_for_each(|msg| {
        debug!("Получено сообщение от: {}: {}", addr, msg.to_text().unwrap());
        let peers = peer_map.lock().unwrap();

        let broadcast_recipients =
            peers.iter().filter(|(peer_addr, _)| peer_addr != &&addr).map(|(_, ws_sink)| ws_sink);

        for recp in broadcast_recipients {
            recp.unbounded_send(msg.clone()).unwrap();
        }

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    debug!("{} отключился", &addr);
    peer_map.lock().unwrap().remove(&addr);

    Ok(())
}



const ip: &str = "193.124.66.129:443";

extern crate env_logger;

#[tokio::main]
async fn main() -> Result<()> {

    std::env::set_var("RUST_LOG", "info");
    env_logger::init();

    let state = PeerMap::new(Mutex::new(HashMap::new()));

    let socket = TcpListener::bind(&ip).await?;
    info!("WebSocket сервер работает: {}", ip);

    while let Ok((stream, addr)) = socket.accept().await {
        tokio::spawn(handle_connection(state.clone(), stream, addr));
        info!("Количество подключённых пользователей: {}", state.lock().unwrap().len() + 1);
    }

    Ok(())
}