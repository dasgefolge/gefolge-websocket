#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        fmt,
        sync::Arc,
        time::Duration,
    },
    derive_more::From,
    futures::{
        SinkExt as _,
        stream::{
            SplitSink,
            Stream,
            StreamExt as _,
            TryStreamExt as _,
        },
    },
    async_proto::{
        Protocol,
        impls::StringReadError,
    },
    gefolge_web::login::Mensch,
    pyo3::PyErr,
    tokio::{
        sync::Mutex,
        time::sleep,
    },
    warp::{
        Filter,
        reject::Rejection,
        reply::Reply,
        ws::{
            Message,
            WebSocket,
        },
    },
};

type WsSink = Arc<Mutex<SplitSink<WebSocket, Message>>>;

#[derive(Debug, From)]
pub enum Error {
    EndOfStream,
    Python(PyErr),
    RicochetRobots(ricochet_robots_websocket::Error),
    StringRead(StringReadError),
    UnknownApiKey,
    Warp(warp::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EndOfStream => write!(f, "reached end of stream"),
            Error::Python(e) => write!(f, "Python error: {}", e),
            Error::RicochetRobots(e) => write!(f, "error in Ricochet Robots session: {}", e),
            Error::StringRead(e) => write!(f, "{:?}", e),
            Error::UnknownApiKey => write!(f, "unknown API key"),
            Error::Warp(e) => e.fmt(f),
        }
    }
}

#[derive(Protocol)]
enum ServerMessage {
    Ping,
    Error {
        debug: String,
        display: String,
    },
}

impl ServerMessage {
    fn from_error(e: impl fmt::Debug + fmt::Display) -> ServerMessage {
        ServerMessage::Error {
            debug: format!("{:?}", e),
            display: e.to_string(),
        }
    }
}

pub async fn client_session(mut stream: impl Stream<Item = Result<Message, warp::Error>> + Unpin, sink: WsSink) -> Result<(), Error> {
    let packet = stream.try_next().await?.ok_or(Error::EndOfStream)?;
    let api_key = String::read(packet.as_bytes()).await?;
    let user = Mensch::by_api_key(&api_key)?.ok_or(Error::UnknownApiKey)?; // verify API key
    let ping_sink = Arc::clone(&sink);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(30)).await;
            let mut buf = Vec::default();
            if ServerMessage::Ping.write(&mut buf).await.is_err() { break } //TODO better error handling
            if ping_sink.lock().await.send(Message::binary(buf)).await.is_err() { break } //TODO better error handling
        }
    });
    ricochet_robots_websocket::client_session(user, stream, sink).await?;
    Ok(())
}

async fn client_connection(ws: WebSocket) {
    let (ws_sink, ws_stream) = ws.split();
    let ws_sink = Arc::new(Mutex::new(ws_sink));
    if let Err(e) = client_session(ws_stream, Arc::clone(&ws_sink)).await {
        let message = ServerMessage::from_error(e);
        let mut buf = Vec::default();
        if message.write(&mut buf).await.is_ok() {
            let _ = ws_sink.lock().await.send(Message::binary(buf)).await;
        }
    }
}

async fn ws_handler(ws: warp::ws::Ws) -> Result<impl Reply, Rejection> {
    Ok(ws.on_upgrade(client_connection))
}

#[tokio::main]
async fn main() {
    let handler = warp::ws().and_then(ws_handler);
    warp::serve(handler).run(([127, 0, 0, 1], 24802)).await;
}
