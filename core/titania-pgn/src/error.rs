use std::fmt;
use std::io;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    /// The caller asked to stop.
    Cancelled,
    Message(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cancelled => f.write_str("Cancelled"),
            Error::Message(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn message(text: impl Into<String>) -> Error {
    Error::Message(text.into())
}

/// An I/O error with the file it happened on, e.g. "Could not open C:\x.pgn: Access is denied."
pub(crate) fn io_error(action: &str, path: &Path, error: io::Error) -> Error {
    Error::Message(format!("Could not {action} {}: {error}", path.display()))
}
