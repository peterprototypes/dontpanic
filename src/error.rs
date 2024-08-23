use std::fmt::{Display, Formatter};

#[cfg(feature = "log")]
use log::SetLoggerError;

/// The Errors that may occur when interacting with this library
#[derive(Debug)]
pub enum Error {
    /// An empty API Key was provided to [`builder`](crate::builder)
    EmptyApiKey,
    /// Error returned by [`set_logger`](crate::Client::set_logger) if another logger has already been set.
    #[doc(cfg(feature = "log"))]
    #[cfg(feature = "log")]
    SetLoggerError(SetLoggerError),
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyApiKey => write!(f, "API Key cannot be empty"),
            #[cfg(feature = "log")]
            Self::SetLoggerError(e) => write!(f, "{}", e),
        }
    }
}

#[cfg(feature = "log")]
impl From<SetLoggerError> for Error {
    fn from(value: SetLoggerError) -> Self {
        Self::SetLoggerError(value)
    }
}
