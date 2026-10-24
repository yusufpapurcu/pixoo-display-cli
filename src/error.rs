use std::{fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Invalid(&'static str),
    Unsupported(&'static str),
    Protocol(&'static str),
    Media(String),
    Io(io::Error),
    Timeout,
    Cancelled,
    Rejected { opcode: u8, status: u8 },
    RetryLimit,
    SessionPoisoned,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid argument: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported: {s}"),
            Self::Protocol(s) => write!(f, "protocol error: {s}"),
            Self::Media(s) => write!(f, "media error: {s}"),
            Self::Io(e) => e.fmt(f),
            Self::Timeout => f.write_str("transaction timed out; delivery may be ambiguous"),
            Self::Cancelled => f.write_str("transaction cancelled; delivery may be ambiguous"),
            Self::Rejected { opcode, status } => {
                write!(f, "command {opcode:#04x} rejected with {status:#04x}")
            }
            Self::RetryLimit => f.write_str("transfer retransmission limit reached"),
            Self::SessionPoisoned => {
                f.write_str("session cannot continue after an ambiguous write or transaction")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
