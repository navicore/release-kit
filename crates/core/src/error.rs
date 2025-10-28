use std::fmt;

#[derive(Debug)]
pub enum Error {
    ConfigParse(String),
    IoError(std::io::Error),
    InvalidData(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ConfigParse(msg) => write!(f, "Configuration parse error: {}", msg),
            Error::IoError(err) => write!(f, "IO error: {}", err),
            Error::InvalidData(msg) => write!(f, "Invalid data: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Error::ConfigParse(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
