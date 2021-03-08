#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        fmt,
        sync::Arc,
        time::Duration,
    },
    derive_more::From,
    futures::stream::{
        SplitSink,
        Stream,
        StreamExt as _,
    },
    async_proto::{
        Protocol,
        ReadError,
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
    Read(ReadError),
    RicochetRobots(ricochet_robots_websocket::Error),
    UnknownApiKey,
    Warp(warp::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EndOfStream => write!(f, "reached end of stream"),
            Error::Python(e) => write!(f, "Python error: {}", e),
            Error::Read(e) => e.fmt(f),
            Error::RicochetRobots(e) => write!(f, "error in Ricochet Robots session: {}", e),
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

#[derive(Protocol)]
enum SessionPurpose {
    RicochetRobots,
}

pub async fn client_session(mut stream: impl Stream<Item = Result<Message, warp::Error>> + Unpin + Send, sink: WsSink) -> Result<(), Error> {
    let api_key = String::read_ws(&mut stream).await?;
    let user = Mensch::by_api_key(&api_key)?.ok_or(Error::UnknownApiKey)?; // verify API key
    let ping_sink = Arc::clone(&sink);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(30)).await;
            if ServerMessage::Ping.write_ws(&mut *ping_sink.lock().await).await.is_err() { break } //TODO better error handling
        }
    });
    match SessionPurpose::read_ws(&mut stream).await? {
        SessionPurpose::RicochetRobots => ricochet_robots_websocket::client_session(user, stream, sink).await?,
    }
    Ok(())
}

async fn client_connection(ws: WebSocket) {
    let (ws_sink, ws_stream) = ws.split();
    let ws_sink = Arc::new(Mutex::new(ws_sink));
    if let Err(e) = client_session(ws_stream, Arc::clone(&ws_sink)).await {
        let _ = ServerMessage::from_error(e).write_ws(&mut *ws_sink.lock().await).await;
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
