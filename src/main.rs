#![deny(rust_2018_idioms, unused, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        fmt,
        sync::Arc,
        time::Duration,
    },
    futures::stream::{
        SplitSink,
        Stream,
        StreamExt as _,
    },
    async_proto::Protocol,
    gefolge_web::login::Mensch,
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
    gefolge_websocket::{
        Error,
        event,
    },
};

type WsSink = Arc<Mutex<SplitSink<WebSocket, Message>>>;

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
    CurrentEvent,
}

pub async fn client_session(event_flow: ctrlflow::Handle<event::Key>, mut stream: impl Stream<Item = Result<Message, warp::Error>> + Unpin + Send, sink: WsSink) -> Result<(), Error> {
    let api_key = String::read_warp(&mut stream).await?;
    let user = Mensch::by_api_key(&api_key)?.ok_or(Error::UnknownApiKey)?; // verify API key TODO allow special device API key for CurrentEvent purpose
    let ping_sink = Arc::clone(&sink);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(30)).await;
            if ServerMessage::Ping.write_warp(&mut *ping_sink.lock().await).await.is_err() { break } //TODO better error handling
        }
    });
    match SessionPurpose::read_warp(&mut stream).await? {
        SessionPurpose::RicochetRobots => ricochet_robots_websocket::client_session(user, stream, sink).await?,
        SessionPurpose::CurrentEvent => match event::client_session(event_flow, sink).await? {},
    }
    Ok(())
}

async fn client_connection(event_flow: ctrlflow::Handle<event::Key>, ws: WebSocket) {
    let (ws_sink, ws_stream) = ws.split();
    let ws_sink = Arc::new(Mutex::new(ws_sink));
    if let Err(e) = client_session(event_flow, ws_stream, Arc::clone(&ws_sink)).await {
        let _ = ServerMessage::from_error(e).write_warp(&mut *ws_sink.lock().await).await;
    }
}

async fn ws_handler(event_flow: ctrlflow::Handle<event::Key>, ws: warp::ws::Ws) -> Result<impl Reply, Rejection> {
    Ok(ws.on_upgrade(|ws| client_connection(event_flow, ws)))
}

#[tokio::main]
async fn main() {
    let event_flow = ctrlflow::run(event::Key).await;
    let handler = warp::ws().and_then(move |ws| ws_handler(event_flow.clone(), ws));
    warp::serve(handler).run(([127, 0, 0, 1], 24802)).await;
}
