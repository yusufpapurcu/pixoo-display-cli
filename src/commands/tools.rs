//! Stopwatch, score, noise, countdown and game controls from pages 264–281.
use super::system::{boolean, checked_reply, le16, put16, Decoded};
use crate::protocol::{Packet, Reply};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ToolKind {
    Stopwatch = 0,
    Score = 1,
    Noise = 2,
    Countdown = 3,
    None = 255,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StopwatchControl {
    Pause = 0,
    Start = 1,
    Reset = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StopwatchStatus {
    Paused = 0,
    Started = 1,
    Reset = 2,
    Entering = 3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NoiseControl {
    Start = 1,
    Stop = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CountdownControl {
    Start = 0,
    Cancel = 1,
    TogglePause = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CountdownStatus {
    Started = 0,
    Cancelled = 1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GameKey {
    Left = 1,
    Right = 2,
    Up = 3,
    Down = 4,
    Ok = 5,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCommand {
    Get(ToolKind),
    Stopwatch(StopwatchControl),
    Score {
        enabled: bool,
        red: u16,
        blue: u16,
    },
    Noise(NoiseControl),
    Countdown {
        control: CountdownControl,
        minutes: u8,
        seconds: u8,
    },
    GameShake,
    GameEnabled(bool),
    KeyDown(GameKey),
    KeyUp(GameKey),
}
impl ToolCommand {
    pub fn packet(&self) -> Result<Packet> {
        let (op, data) = match self {
            Self::Get(v) => (0x71, vec![*v as u8]),
            Self::Stopwatch(v) => (0x72, vec![0, *v as u8]),
            Self::Score { enabled, red, blue } => {
                let mut b = vec![1, u8::from(*enabled)];
                put16(&mut b, *red);
                put16(&mut b, *blue);
                (0x72, b)
            }
            Self::Noise(v) => (0x72, vec![2, *v as u8]),
            Self::Countdown {
                control,
                minutes,
                seconds,
            } => {
                if *seconds > 59 {
                    return Err(Error::Invalid("countdown seconds must be 0..59"));
                }
                (0x72, vec![3, *control as u8, *minutes, *seconds])
            }
            Self::GameShake => (0x88, vec![]),
            Self::GameEnabled(v) => (0xa0, vec![u8::from(*v)]),
            Self::KeyDown(v) => (0x17, vec![*v as u8]),
            Self::KeyUp(v) => (0x21, vec![*v as u8]),
        };
        Packet::new(op, data)
    }
    pub fn response_opcode(&self) -> Option<u8> {
        match self {
            Self::Get(_) => Some(0x71),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolReply {
    Stopwatch(StopwatchStatus),
    Score {
        enabled: bool,
        red: u16,
        blue: u16,
    },
    Noise(NoiseControl),
    Countdown {
        status: CountdownStatus,
        minutes: u8,
        seconds: u8,
    },
    None,
}
impl ToolReply {
    /// The source defines no kind byte in replies. The requested kind supplies it.
    pub fn decode(reply: &Reply, kind: ToolKind) -> Result<Decoded<Self>> {
        let needed = match kind {
            ToolKind::Stopwatch | ToolKind::Noise => 1,
            ToolKind::Score => 5,
            ToolKind::Countdown => 3,
            ToolKind::None => 0,
        };
        let d = checked_reply(reply, 0x71, needed)?;
        let value = match kind {
            ToolKind::Stopwatch => Self::Stopwatch(match d[0] {
                0 => StopwatchStatus::Paused,
                1 => StopwatchStatus::Started,
                2 => StopwatchStatus::Reset,
                3 => StopwatchStatus::Entering,
                _ => return Err(Error::Protocol("invalid stopwatch status")),
            }),
            ToolKind::Score => Self::Score {
                enabled: boolean(d[0])?,
                red: le16(&d[1..]),
                blue: le16(&d[3..]),
            },
            ToolKind::Noise => Self::Noise(match d[0] {
                1 => NoiseControl::Start,
                2 => NoiseControl::Stop,
                _ => return Err(Error::Protocol("invalid noise status")),
            }),
            ToolKind::Countdown => {
                if d[2] > 59 {
                    return Err(Error::Protocol("invalid countdown seconds"));
                }
                Self::Countdown {
                    status: match d[0] {
                        0 => CountdownStatus::Started,
                        1 => CountdownStatus::Cancelled,
                        _ => return Err(Error::Protocol("invalid countdown status")),
                    },
                    minutes: d[1],
                    seconds: d[2],
                }
            }
            ToolKind::None => Self::None,
        };
        Ok(Decoded {
            value,
            extension: d[needed..].to_vec(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameProgress {
    pub game_type: u8,
    pub seconds: u16,
    pub score: u16,
}
impl GameProgress {
    /// Page 280 has no acknowledgement byte. Pass the five-byte notification
    /// body after the command byte directly, without stripping its game type.
    pub fn decode_event(opcode: u8, data: &[u8]) -> Result<Decoded<Self>> {
        if opcode != 0x17 {
            return Err(Error::Protocol("unexpected game event opcode"));
        }
        if data.len() < 5 {
            return Err(Error::Protocol("truncated game progress"));
        }
        Ok(Decoded {
            value: Self {
                game_type: data[0],
                seconds: le16(&data[1..]),
                score: le16(&data[3..]),
            },
            extension: data[5..].to_vec(),
        })
    }
}
impl super::Command for ToolCommand {
    fn packet(&self) -> Result<Packet> {
        ToolCommand::packet(self)
    }
    fn response_opcode(&self) -> Option<u8> {
        ToolCommand::response_opcode(self)
    }
}
