use {
    std::{
        collections::HashSet,
        convert::{
            Infallible as Never,
            TryInto as _,
        },
        ffi::OsString,
        io,
        iter,
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
    git2::Repository,
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

pub struct State {
    event_id: Option<String>,
    latest_version: [u8; 20],
}

impl State {
    fn to_init_deltas(&self) -> impl Iterator<Item = Delta> {
        iter::once(Delta::LatestVersion(self.latest_version))
            .chain(iter::once(if let Some(ref event_id) = self.event_id { Delta::CurrentEvent(event_id.clone()) } else { Delta::NoEvent }))
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
    LatestVersion([u8; 20]),
}

impl ctrlflow::Delta<Result<State, Error>> for Delta {
    fn apply(&self, state: &mut Result<State, Error>) {
        if let Ok(state) = state {
            match self {
                Delta::Ping => {}
                Delta::Error { display, .. } => panic!("tried to apply error delta: {}", display),
                Delta::NoEvent => state.event_id = None,
                Delta::CurrentEvent(id) => state.event_id = Some(id.clone()),
                Delta::LatestVersion(commit_hash) => state.latest_version = *commit_hash,
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key;

async fn init(mut events_dir_states: Pin<&mut impl Stream<Item = Result<HashSet<OsString>, Arc<io::Error>>>>) -> Result<State, Error> {
    let now = Utc::now();
    let init = events_dir_states.next().await.expect("empty dir states stream").at(DATA_PATH)?;
    let mut current_event = None;
    for filename in init {
        let event_id = filename.into_string()?.strip_suffix(".json").ok_or(Error::NonJsonEventFile)?.to_owned();
        let event = Event::load(event_id).await?;
        if let (Some(start), Some(end)) = (event.start().await?, event.end().await?) {
            if start <= now && now < end {
                if current_event.is_none() {
                    current_event = Some(event);
                } else {
                    return Err(Error::MultipleCurrentEvents)
                }
            }
        }
    }
    Ok(State {
        event_id: current_event.map(|event| event.id),
        latest_version: Repository::open("/opt/git/github.com/dasgefolge/sil/master")?.head()?.peel_to_commit()?.id().as_bytes().try_into()?,
    })
}

impl ctrlflow::Key for Key {
    type State = Result<State, Error>;
    type Delta = Delta;

    fn maintain(self, runner: ctrlflow::RunnerInternal) -> Pin<Box<dyn Future<Output = (Result<State, Error>, Pin<Box<dyn Stream<Item = Delta> + Send + 'static>>)> + Send + 'static>> {
        Box::pin(async move {
            let events_path = Path::new(DATA_PATH);
            let events_dir = runner.subscribe(ctrlflow::fs::Dir(events_path.to_owned())).await.expect("dependency loop");
            let events_dir_states = events_dir.states();
            pin_mut!(events_dir_states);
            match init(events_dir_states).await {
                Ok(init) => (Ok(init), Box::pin(stream::empty()) as Pin<Box<dyn Stream<Item = Delta> + Send + 'static>>), //TODO update current event if events dir or any file contents change or at the end of the current event; update latest version as a gitdir post-deploy hook (after making sure it has been built on reiwa)
                Err(e) => (Err(e), Box::pin(stream::empty()) as Pin<Box<dyn Stream<Item = Delta> + Send + 'static>>),
            }
        })
    }
}

type WsSink = Arc<Mutex<SplitSink<WebSocket, Message>>>;

pub async fn client_session(flow: ctrlflow::Handle<Key>, sink: WsSink) -> Result<Never, Error> {
    let (init, mut deltas) = flow.stream().await;
    match *init {
        Ok(ref state) => for delta in state.to_init_deltas() {
            delta.write_warp(&mut *sink.lock().await).await?;
        },
        Err(ref e) => {
            let delta = Delta::Error { debug: format!("{:?}", e), display: e.to_string() };
            delta.write_warp(&mut *sink.lock().await).await?;
        }
    }
    loop {
        let delta = deltas.recv().await?;
        delta.write_warp(&mut *sink.lock().await).await?;
    }
}
