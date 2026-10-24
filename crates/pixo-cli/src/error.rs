use serde_json::{json, Value};
use std::fmt;

pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug)]
pub struct CliError {
    pub category: &'static str,
    pub message: String,
    pub code: u8,
    pub uncertain: bool,
    pub completed: Vec<Value>,
    pub device: Option<String>,
}

impl CliError {
    pub fn new(category: &'static str, code: u8, message: impl Into<String>) -> Self {
        Self {
            category,
            code,
            message: message.into(),
            uncertain: false,
            completed: Vec::new(),
            device: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", 2, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new("unsupported", 4, message)
    }
    pub fn value(&self) -> Value {
        json!({"category": self.category, "message": self.message,
            "delivery_uncertain": self.uncertain, "completed_steps": self.completed})
    }
}
impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for CliError {}
impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::new("local_io", 1, e.to_string())
    }
}
impl From<divoom_bluetooth::Error> for CliError {
    fn from(e: divoom_bluetooth::Error) -> Self {
        use divoom_bluetooth::Error::*;
        let (category, code) = match &e {
            Invalid(_) | Media(_) => ("invalid_input", 2),
            Unsupported(_) => ("unsupported", 4),
            Timeout => ("timeout", 7),
            Cancelled => ("cancelled", 130),
            Rejected { .. } => ("device_rejected", 6),
            SessionPoisoned => ("session_poisoned", 6),
            _ => ("protocol", 6),
        };
        Self::new(category, code, e.to_string())
    }
}
