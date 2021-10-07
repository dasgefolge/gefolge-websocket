use {
    std::{
        convert::Infallible as Never,
        path::Path,
        pin::Pin,
        sync::Arc,
    },
    async_proto::Protocol,
    chrono::prelude::*,
    chrono_tz::Tz,
    futures::{
        future::Future,
        pin_mut,
        stream::{
            self,
            SplitSink,
            Stream,
            StreamExt as _,
        },
    },
    serde::Deserialize,
    tokio::{
        fs,
        sync::Mutex,
    },
    warp::ws::{
        Message,
        WebSocket,
    },
    crate::{
        Error,
        IntoResult as _,
        IoResultExt as _,
    },
};

const DATA_PATH: &str = "/usr/local/share/fidera/event";
const LOCATIONS_PATH: &str = "/usr/local/share/fidera/loc";

#[derive(Deserialize)]
struct Location {
    #[serde(skip)]
    id: String,
    timezone: Tz,
}

impl Location {
    async fn load(loc_id: String) -> Result<Self, Error> {
        let loc_path = Path::new(LOCATIONS_PATH).join(format!("{}.json", loc_id));
        let buf = fs::read_to_string(&loc_path).await.at(&loc_path)?;
        let mut location = serde_json::from_str::<Self>(&buf).at(loc_path)?;
        location.id = loc_id;
        Ok(location)
    }
}

enum LocationInfo {
    Unknown,
    Online,
    Known(Location),
}

impl LocationInfo {
    fn timezone(&self) -> Tz {
        match self {
            Self::Unknown | Self::Online => chrono_tz::Europe::Berlin,
            Self::Known(location) => location.timezone,
        }
    }
}

#[derive(Deserialize)]
struct Event {
    #[serde(skip)]
    id: String,
    end: Option<NaiveDateTime>,
    location: Option<String>,
    start: Option<NaiveDateTime>,
    timezone: Option<Tz>,
}

impl Event {
    async fn load(event_id: String) -> Result<Self, Error> {
        let event_path = Path::new(DATA_PATH).join(format!("{}.json", event_id));
        let buf = fs::read_to_string(&event_path).await.at(&event_path)?;
        let mut event = serde_json::from_str::<Self>(&buf).at(event_path)?;
        event.id = event_id;
        Ok(event)
    }

    async fn location_info(&self) -> Result<LocationInfo, Error> {
        Ok(match self.location.as_deref() {
            Some("online") => LocationInfo::Online,
            Some(name) => LocationInfo::Known(Location::load(name.to_owned()).await?),
            None => LocationInfo::Unknown
        })
    }

    async fn start(&self) -> Result<Option<DateTime<Tz>>, Error> {
        Ok(if let Some(start_naive) = self.start {
            let timezone = if let Some(timezone) = self.timezone {
                timezone
            } else {
                self.location_info().await?.timezone()
            };
            Some(timezone.from_local_datetime(&start_naive).into_result()?)
        } else {
            None
        })
    }

    async fn end(&self) -> Result<Option<DateTime<Tz>>, Error> {
        Ok(if let Some(end_naive) = self.end {
            let timezone = if let Some(timezone) = self.timezone {
                timezone
            } else {
                self.location_info().await?.timezone()
            };
            Some(timezone.from_local_datetime(&end_naive).into_result()?)
        } else {
            None
        })
    }
}

pub struct State(Result<Option<String>, Error>);

impl State {
    fn to_init_delta(&self) -> Delta {
        match self {
            State(Ok(Some(id))) => Delta::CurrentEvent(id.clone()),
            State(Ok(None)) => Delta::NoEvent,
            State(Err(e)) => Delta::Error {
                debug: format!("{:?}", e),
                display: e.to_string(),
            },
        }
    }
}

#[derive(Clone, Protocol)]
pub enum Delta {
    Ping,
    Error {
        debug: String,
        display: String,
    },
    NoEvent,
    CurrentEvent(String),
}

impl ctrlflow::Delta<State> for Delta {
    fn apply(&self, state: &mut State) {
        if let Ok(ref mut state) = state.0 {
            match self {
                Delta::Ping => {}
                Delta::Error { display, .. } => panic!("tried to apply error delta: {}", display),
                Delta::NoEvent => *state = None,
                Delta::CurrentEvent(id) => *state = Some(id.clone()),
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key;

impl ctrlflow::Key for Key {
    type State = State;
    type Delta = Delta;

    fn maintain(self, runner: ctrlflow::RunnerInternal) -> Pin<Box<dyn Future<Output = (State, Pin<Box<dyn Stream<Item = Delta> + Send + 'static>>)> + Send + 'static>> {
        macro_rules! try_init {
            ($e:expr) => {
                match $e {
                    Ok(value) => value,
                    Err(e) => return (State(Err(e.into())), Box::pin(stream::empty()) as Pin<Box<dyn Stream<Item = Delta> + Send + 'static>>),
                }
            };
            (@io $e:expr) => { try_init!($e.at_unknown()) };
        }

        Box::pin(async move {
            let now = Utc::now();
            let events_path = Path::new(DATA_PATH);
            let events_dir = runner.subscribe(ctrlflow::fs::Dir(events_path.to_owned())).await.expect("dependency loop");
            let events_dir_states = events_dir.states();
            pin_mut!(events_dir_states);
            let init = try_init!(@io events_dir_states.next().await.expect("empty dir states stream"));
            let mut current_event = None;
            for filename in init {
                let event_id = try_init!(try_init!(filename.into_string()).strip_suffix(".json").ok_or(Error::NonJsonEventFile)).to_owned();
                let event = try_init!(Event::load(event_id).await);
                if let (Some(start), Some(end)) = (try_init!(event.start().await), try_init!(event.end().await)) {
                    if start <= now && now < end {
                        if current_event.is_none() {
                            current_event = Some(event);
                        } else {
                            try_init!(Err(Error::MultipleCurrentEvents))
                        }
                    }
                }
            }
            (State(Ok(current_event.map(|event| event.id))), Box::pin(stream::empty())) //TODO periodically update current event
        })
    }
}

type WsSink = Arc<Mutex<SplitSink<WebSocket, Message>>>;

pub async fn client_session(flow: ctrlflow::Handle<Key>, sink: WsSink) -> Result<Never, Error> {
    let (init, mut deltas) = flow.stream().await;
    init.to_init_delta().write_warp(&mut *sink.lock().await).await?;
    loop {
        let delta = deltas.recv().await?;
        delta.write_warp(&mut *sink.lock().await).await?;
    }
}
