#![deny(rust_2018_idioms, unused, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        array,
        ffi::OsString,
        fmt,
        io,
        path::{
            Path,
            PathBuf,
        },
        sync::Arc,
    },
    async_proto::ReadError,
    chrono::{
        offset::LocalResult,
        prelude::*,
    },
    chrono_tz::Tz,
    derive_more::From,
    pyo3::PyErr,
    wheel::FromArc,
};

pub mod event;

#[derive(Debug, From, FromArc)]
pub enum Error {
    #[from(ignore)]
    AmbiguousTimestamp(DateTime<Tz>, DateTime<Tz>),
    ArrayFromSlice(array::TryFromSliceError),
    Broadcast(tokio::sync::broadcast::error::RecvError),
    EndOfStream,
    Git(git2::Error),
    InvalidTimestamp,
    Io(Arc<io::Error>, Option<PathBuf>),
    Json(serde_json::Error, Option<PathBuf>),
    MultipleCurrentEvents,
    NonJsonEventFile,
    OsString(OsString),
    Python(PyErr),
    Read(ReadError),
    RicochetRobots(ricochet_robots_websocket::Error),
    UnknownApiKey,
    Warp(warp::Error),
    Write(async_proto::WriteError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AmbiguousTimestamp(dt1, dt2) => write!(f, "ambiguous timestamp: could refer to {} or {} UTC", dt1.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S"), dt2.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S")),
            Error::ArrayFromSlice(e) => e.fmt(f),
            Error::Broadcast(e) => e.fmt(f),
            Error::EndOfStream => write!(f, "reached end of stream"),
            Error::Git(e) => write!(f, "git error: {}", e),
            Error::InvalidTimestamp => write!(f, "invalid timestamp"),
            Error::Io(e, Some(path)) => write!(f, "I/O error at {}: {}", path.display(), e),
            Error::Io(e, None) => write!(f, "I/O error: {}", e),
            Error::Json(e, Some(path)) => write!(f, "JSON error at {}: {}", path.display(), e),
            Error::Json(e, None) => write!(f, "JSON error: {}", e),
            Error::MultipleCurrentEvents => write!(f, "there are multiple events currently ongoing"),
            Error::NonJsonEventFile => write!(f, "events dir contains a non-.json file"),
            Error::OsString(_) => write!(f, "filename was not valid Unicode"),
            Error::Python(e) => write!(f, "Python error: {}", e),
            Error::Read(e) => e.fmt(f),
            Error::RicochetRobots(e) => write!(f, "error in Ricochet Robots session: {}", e),
            Error::UnknownApiKey => write!(f, "unknown API key"),
            Error::Warp(e) => e.fmt(f),
            Error::Write(e) => write!(f, "error writing to websocket: {}", e),
        }
    }
}

trait IntoResult<T> {
    fn into_result(self) -> Result<T, Error>;
}

impl IntoResult<DateTime<Tz>> for LocalResult<DateTime<Tz>> {
    fn into_result(self) -> Result<DateTime<Tz>, Error> {
        match self {
            Self::None => Err(Error::InvalidTimestamp),
            Self::Single(dt) => Ok(dt),
            Self::Ambiguous(dt1, dt2) => Err(Error::AmbiguousTimestamp(dt1, dt2)),
        }
    }
}

trait IoResultExt {
    type T;

    fn at(self, path: impl AsRef<Path>) -> Result<Self::T, Error>;
    fn at_unknown(self) -> Result<Self::T, Error>;
}

impl<T> IoResultExt for io::Result<T> {
    type T = T;

    fn at(self, path: impl AsRef<Path>) -> Result<T, Error> {
        self.map_err(|e| Error::Io(Arc::new(e), Some(path.as_ref().to_owned())))
    }

    fn at_unknown(self) -> Result<T, Error> {
        self.map_err(|e| Error::Io(Arc::new(e), None))
    }
}

impl<T> IoResultExt for Result<T, Arc<io::Error>> {
    type T = T;

    fn at(self, path: impl AsRef<Path>) -> Result<T, Error> {
        self.map_err(|e| Error::Io(e, Some(path.as_ref().to_owned())))
    }

    fn at_unknown(self) -> Result<T, Error> {
        self.map_err(|e| Error::Io(e, None))
    }
}

impl<T> IoResultExt for serde_json::Result<T> {
    type T = T;

    fn at(self, path: impl AsRef<Path>) -> Result<T, Error> {
        self.map_err(|e| Error::Json(e, Some(path.as_ref().to_owned())))
    }

    fn at_unknown(self) -> Result<T, Error> {
        self.map_err(|e| Error::Json(e, None))
    }
}
