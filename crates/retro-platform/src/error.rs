use std::fmt;

/// Errors produced by platform backends.
#[derive(Debug)]
pub enum PlatformError {
    /// The backend was used before `Platform::init`.
    NotInitialized,
    /// The requested capability is not available in this backend or build.
    Unsupported(String),
    /// A caller supplied an invalid argument.
    InvalidArgument(String),
    /// An SDL3 call failed.
    Sdl(String),
    /// A filesystem operation failed.
    Io(std::io::Error),
    /// Any other backend failure.
    Other(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "platform is not initialized"),
            Self::Unsupported(what) => write!(f, "unsupported platform operation: {what}"),
            Self::InvalidArgument(what) => write!(f, "invalid platform argument: {what}"),
            Self::Sdl(message) => write!(f, "SDL error: {message}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Other(message) => write!(f, "platform error: {message}"),
        }
    }
}

impl std::error::Error for PlatformError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PlatformError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl PlatformError {
    /// Wraps any displayable backend error message.
    pub fn other(message: impl fmt::Display) -> Self {
        Self::Other(message.to_string())
    }

    /// Wraps an SDL3 error message.
    pub fn sdl(message: impl fmt::Display) -> Self {
        Self::Sdl(message.to_string())
    }
}
