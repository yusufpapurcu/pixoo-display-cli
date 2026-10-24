//! System, weather and SD playback commands from official pages 147 and 178–214.
//! Unicode SD names retain their bytes because the pages do not specify a consistent
//! encoding byte order. Fixed-width integers use little endian where documented.
use super::{ClockTime, Percent};
use crate::protocol::{Packet, Reply};
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded<T> {
    pub value: T,
    pub extension: Vec<u8>,
}

pub(crate) fn checked_reply(reply: &Reply, opcode: u8, minimum: usize) -> Result<&[u8]> {
    if reply.opcode != opcode {
        return Err(Error::Protocol("unexpected reply opcode"));
    }
    reply.ensure_success()?;
    if reply.data.len() < minimum {
        return Err(Error::Protocol("truncated command reply"));
    }
    Ok(&reply.data)
}
pub(crate) fn boolean(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Protocol("invalid boolean")),
    }
}
pub(crate) fn le16(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}
pub(crate) fn put16(data: &mut Vec<u8>, value: u16) {
    data.extend_from_slice(&value.to_le_bytes());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkMode {
    Bluetooth = 0,
    Fm = 1,
    LineIn = 2,
    Sd = 3,
    UsbHost = 4,
    Record = 5,
    RecordPlay = 6,
    Uac = 7,
    Phone = 8,
    DivoomShow = 9,
    AlarmSet = 10,
    Game = 11,
}
impl TryFrom<u8> for WorkMode {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::Bluetooth,
            1 => Self::Fm,
            2 => Self::LineIn,
            3 => Self::Sd,
            4 => Self::UsbHost,
            5 => Self::Record,
            6 => Self::RecordPlay,
            7 => Self::Uac,
            8 => Self::Phone,
            9 => Self::DivoomShow,
            10 => Self::AlarmSet,
            11 => Self::Game,
            _ => return Err(Error::Protocol("unknown work mode")),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TemperatureUnit {
    Celsius = 0,
    Fahrenheit = 1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HourFormat {
    Twelve = 0,
    TwentyFour = 1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting<T> {
    Get,
    Set(T),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerOnChannel {
    TimeFace = 0,
    Nightlight = 1,
    Cloud = 2,
    Vj = 3,
    MusicEq = 4,
    Custom = 5,
}
impl TryFrom<u8> for PowerOnChannel {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::TimeFace,
            1 => Self::Nightlight,
            2 => Self::Cloud,
            3 => Self::Vj,
            4 => Self::MusicEq,
            5 => Self::Custom,
            _ => return Err(Error::Protocol("unknown power-on channel")),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayMode {
    ListLoop = 1,
    SingleLoop = 2,
    Shuffle = 3,
}
impl TryFrom<u8> for PlayMode {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        match v {
            1 => Ok(Self::ListLoop),
            2 => Ok(Self::SingleLoop),
            3 => Ok(Self::Shuffle),
            _ => Err(Error::Protocol("invalid play mode")),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume(u8);
impl Volume {
    pub fn new(value: u8) -> Result<Self> {
        if value > 15 {
            Err(Error::Invalid("volume must be 0..15"))
        } else {
            Ok(Self(value))
        }
    }
    pub fn get(self) -> u8 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weather {
    pub temperature: i8,
    pub kind: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeatherReport {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub time: ClockTime,
    pub weather: Vec<Weather>,
}
pub(crate) fn valid_month_day(month: u8, day: u8) -> Result<()> {
    let maximum = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29,
        _ => return Err(Error::Invalid("month must be 1..12")),
    };
    if day == 0 || day > maximum {
        return Err(Error::Invalid("invalid day of month"));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeatherDisplay {
    pub modes: [bool; 5],
    pub minute_of_day: u16,
}
impl WeatherDisplay {
    fn bytes(&self) -> Result<Vec<u8>> {
        if self.minute_of_day >= 1440 {
            return Err(Error::Invalid("minute of day must be below 1440"));
        }
        let mut b = self.modes.map(u8::from).to_vec();
        put16(&mut b, self.minute_of_day);
        Ok(b)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasswordControl {
    Clear,
    Set {
        digits: [u8; 4],
        encoding: PasswordEncoding,
    },
    Get,
}
/// Page 194 does not distinguish numeric digit bytes from ASCII digit bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordEncoding {
    NumericDigits,
    AsciiDigits,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdPlayback {
    pub position_seconds: u16,
    pub music_id: u16,
    pub volume: Volume,
    pub playing: bool,
    pub mode: PlayMode,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemCommand {
    Brightness(Percent),
    WorkMode(WorkMode),
    GetWorkMode,
    GetSdStatus,
    GetDeviceTemperature,
    NetworkWeather(WeatherReport),
    WeatherDisplay(WeatherDisplay),
    CurrentWeather(Weather),
    GetWeatherDisplay,
    DeviceName(String),
    GetDeviceName,
    LowPower(bool),
    GetLowPower,
    TemperatureUnit(Setting<TemperatureUnit>),
    HourFormat(Setting<HourFormat>),
    SongNameDisplay(Setting<bool>),
    Password(PasswordControl),
    PowerOnVolume(Setting<Percent>),
    PowerOnChannel(Setting<PowerOnChannel>),
    AutoPowerOff(u16),
    GetAutoPowerOff,
    GetSdTrackName,
    SdMusicList { start: u16, end: u16 },
    Volume(Volume),
    GetVolume,
    Playing(bool),
    GetPlaying,
    SdTrack(u16),
    NextTrack(bool),
    GetSdTrackCount,
    GetSdPlayback,
    SdPlayback(SdPlayback),
    SdPosition(u16),
    SdPlayMode(PlayMode),
    RequestMusicList,
    SoundControl(bool),
    GetSoundControl,
}
impl SystemCommand {
    pub fn response_opcode(&self) -> Option<u8> {
        Some(match self {
            Self::GetWorkMode => 0x13,
            Self::GetSdStatus => 0x15,
            Self::GetDeviceTemperature => 0x59,
            Self::GetWeatherDisplay => 0x73,
            Self::GetDeviceName => 0x76,
            Self::GetLowPower => 0xb3,
            Self::TemperatureUnit(Setting::Get) => 0x2b,
            Self::HourFormat(Setting::Get) => 0x2c,
            Self::SongNameDisplay(Setting::Get) => 0x83,
            Self::Password(_) => 0x27,
            Self::PowerOnVolume(_) => 0xbb,
            Self::PowerOnChannel(_) => 0x8a,
            Self::GetAutoPowerOff => 0xac,
            Self::GetSdTrackName => 6,
            Self::SdMusicList { .. } => 7,
            Self::GetVolume => 9,
            Self::GetPlaying => 11,
            Self::SdTrack(_) => 0x11,
            Self::GetSdTrackCount => 0x7d,
            Self::GetSdPlayback => 0xb4,
            Self::RequestMusicList => 0x47,
            Self::GetSoundControl => 0xa8,
            _ => return None,
        })
    }
    pub fn packet(&self) -> Result<Packet> {
        let (op, data) = match self {
            Self::Brightness(v) => (0x74, vec![v.get()]),
            Self::WorkMode(v) => (5, vec![*v as u8]),
            Self::GetWorkMode => (0x13, vec![]),
            Self::GetSdStatus => (4, vec![0x15]),
            Self::GetDeviceTemperature => (0x59, vec![]),
            Self::NetworkWeather(v) => {
                valid_month_day(v.month, v.day)?;
                if v.month == 2
                    && v.day == 29
                    && !(v.year % 4 == 0 && (v.year % 100 != 0 || v.year % 400 == 0))
                {
                    return Err(Error::Invalid("invalid leap day"));
                }
                let count = u8::try_from(v.weather.len())
                    .map_err(|_| Error::Invalid("too many weather entries"))?;
                let mut b = v.year.to_le_bytes().to_vec();
                b.extend_from_slice(&[v.month, v.day, v.time.hour(), v.time.minute(), count]);
                for w in &v.weather {
                    b.extend_from_slice(&[w.temperature as u8, w.kind]);
                }
                (0x5d, b)
            }
            Self::WeatherDisplay(v) => (0x5e, v.bytes()?),
            Self::CurrentWeather(v) => (0x5f, vec![v.temperature as u8, v.kind]),
            Self::GetWeatherDisplay => (0x73, vec![]),
            Self::DeviceName(v) => {
                if v.chars().count() > 16 {
                    return Err(Error::Invalid("device name exceeds 16 characters"));
                }
                let n =
                    u8::try_from(v.len()).map_err(|_| Error::Invalid("device name too long"))?;
                let mut b = vec![n];
                b.extend_from_slice(v.as_bytes());
                (0x75, b)
            }
            Self::GetDeviceName => (0x76, vec![]),
            Self::LowPower(v) => (0xb2, vec![u8::from(*v)]),
            Self::GetLowPower => (0xb3, vec![]),
            Self::TemperatureUnit(v) => (
                0x2b,
                vec![match v {
                    Setting::Get => 255,
                    Setting::Set(v) => *v as u8,
                }],
            ),
            Self::HourFormat(v) => (
                0x2c,
                vec![match v {
                    Setting::Get => 255,
                    Setting::Set(v) => *v as u8,
                }],
            ),
            Self::SongNameDisplay(v) => (
                0x83,
                vec![match v {
                    Setting::Get => 255,
                    Setting::Set(v) => u8::from(*v),
                }],
            ),
            Self::Password(v) => {
                let b = match v {
                    PasswordControl::Clear => vec![0],
                    PasswordControl::Get => vec![2],
                    PasswordControl::Set { digits, encoding } => {
                        if digits.iter().any(|d| *d > 9) {
                            return Err(Error::Invalid("password digits must be 0..9"));
                        }
                        let mut b = vec![1];
                        b.extend(digits.iter().map(|d| {
                            d + match encoding {
                                PasswordEncoding::NumericDigits => 0,
                                PasswordEncoding::AsciiDigits => b'0',
                            }
                        }));
                        b
                    }
                };
                (0x27, b)
            }
            Self::PowerOnVolume(v) => (
                0xbb,
                match v {
                    Setting::Get => vec![0, 0],
                    Setting::Set(v) => vec![1, v.get()],
                },
            ),
            Self::PowerOnChannel(v) => (
                0x8a,
                match v {
                    Setting::Get => vec![0, 0],
                    Setting::Set(v) => vec![1, *v as u8],
                },
            ),
            Self::AutoPowerOff(v) => (0xab, v.to_le_bytes().to_vec()),
            Self::GetAutoPowerOff => (0xac, vec![]),
            Self::GetSdTrackName => (6, vec![]),
            Self::SdMusicList { start, end } => {
                if start > end {
                    return Err(Error::Invalid("music list range is reversed"));
                }
                let mut b = start.to_le_bytes().to_vec();
                put16(&mut b, *end);
                (7, b)
            }
            Self::Volume(v) => (8, vec![v.get()]),
            Self::GetVolume => (9, vec![]),
            Self::Playing(v) => (10, vec![u8::from(*v)]),
            Self::GetPlaying => (11, vec![]),
            Self::SdTrack(v) => (0x11, v.to_le_bytes().to_vec()),
            Self::NextTrack(v) => (0x12, vec![u8::from(*v)]),
            Self::GetSdTrackCount => (0x7d, vec![]),
            Self::GetSdPlayback => (0xb4, vec![]),
            Self::SdPlayback(v) => {
                let mut b = v.position_seconds.to_le_bytes().to_vec();
                put16(&mut b, v.music_id);
                b.extend_from_slice(&[v.volume.get(), u8::from(v.playing), v.mode as u8]);
                (0xb5, b)
            }
            Self::SdPosition(v) => (0xb8, v.to_le_bytes().to_vec()),
            Self::SdPlayMode(v) => (0xb9, vec![*v as u8]),
            Self::RequestMusicList => (0x47, vec![]),
            Self::SoundControl(v) => (0xa7, vec![u8::from(*v)]),
            Self::GetSoundControl => (0xa8, vec![]),
        };
        Packet::new(op, data)
    }
}

/// Page 146 defines little-endian integers. An explicit override supports other dialects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferLengthOrder {
    LittleEndian,
    BigEndian,
}
pub(crate) fn animation_packets(
    op: u8,
    selector: u8,
    data: &[u8],
    order: TransferLengthOrder,
) -> Result<Vec<Packet>> {
    if data.is_empty() || data.len() > 200 * 256 {
        return Err(Error::Invalid("animation must contain 1..51200 bytes"));
    }
    let length = u16::try_from(data.len()).map_err(|_| Error::Invalid("animation is too large"))?;
    let length = match order {
        TransferLengthOrder::LittleEndian => length.to_le_bytes(),
        TransferLengthOrder::BigEndian => length.to_be_bytes(),
    };
    data.chunks(200)
        .enumerate()
        .map(|(id, chunk)| {
            let mut b = vec![selector, length[0], length[1], id as u8];
            b.extend_from_slice(chunk);
            Packet::new(op, b)
        })
        .collect()
}
pub fn boot_animation_packets(enabled: bool, encoded: &[u8]) -> Result<Vec<Packet>> {
    animation_packets(
        0x52,
        u8::from(enabled),
        encoded,
        TransferLengthOrder::LittleEndian,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdName {
    pub music_id: Option<u16>,
    pub unicode_bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemReply {
    WorkMode(WorkMode),
    SdInserted(bool),
    DeviceTemperature {
        unit: TemperatureUnit,
        value: i8,
    },
    WeatherDisplay(WeatherDisplay),
    DeviceName(String),
    LowPower(bool),
    TemperatureUnit(TemperatureUnit),
    HourFormat(HourFormat),
    SongNameDisplay(bool),
    Password(Option<[u8; 4]>),
    PowerOnVolume(Percent),
    PowerOnChannel(PowerOnChannel),
    AutoPowerOff(u16),
    SdName(SdName),
    Volume(Volume),
    Playing(bool),
    SdListComplete,
    SdTrackCount(u16),
    SdPlayback {
        playback: SdPlayback,
        total_seconds: u16,
    },
    MusicListReady,
    SoundControl(bool),
    Acknowledged(u8),
}
impl SystemReply {
    /// Decode a notification whose wire format omits acknowledgement status.
    pub fn decode_event(opcode: u8, data: &[u8]) -> Result<Decoded<Self>> {
        match opcode {
            0x13 => {
                if data.is_empty() {
                    return Err(Error::Protocol("truncated work mode event"));
                }
                Ok(Decoded {
                    value: Self::WorkMode(WorkMode::try_from(data[0])?),
                    extension: data[1..].to_vec(),
                })
            }
            0x14 => Ok(Decoded {
                value: Self::SdListComplete,
                extension: data.to_vec(),
            }),
            _ => Err(Error::Unsupported(
                "no documented status-less system event schema",
            )),
        }
    }
    pub fn decode(reply: &Reply) -> Result<Decoded<Self>> {
        Self::decode_inner(reply, Some(TransferLengthOrder::LittleEndian))
    }
    /// Override page 146 little-endian integer order for a device dialect.
    pub fn decode_with_name_length_order(
        reply: &Reply,
        order: TransferLengthOrder,
    ) -> Result<Decoded<Self>> {
        Self::decode_inner(reply, Some(order))
    }
    fn decode_inner(
        reply: &Reply,
        name_length_order: Option<TransferLengthOrder>,
    ) -> Result<Decoded<Self>> {
        reply.ensure_success()?;
        let d = &reply.data;
        let one = || checked_reply(reply, reply.opcode, 1).map(|b| b[0]);
        let (value, used) = match reply.opcode {
            0x13 => (Self::WorkMode(WorkMode::try_from(one()?)?), 1),
            0x15 => (Self::SdInserted(boolean(one()?)?), 1),
            0x59 => {
                checked_reply(reply, 0x59, 2)?;
                (
                    Self::DeviceTemperature {
                        unit: unit(d[0])?,
                        value: d[1] as i8,
                    },
                    2,
                )
            }
            0x73 => {
                checked_reply(reply, 0x73, 7)?;
                let mut modes = [false; 5];
                for i in 0..5 {
                    modes[i] = boolean(d[i])?;
                }
                let minute_of_day = le16(&d[5..]);
                if minute_of_day >= 1440 {
                    return Err(Error::Protocol("invalid minute of day"));
                }
                (
                    Self::WeatherDisplay(WeatherDisplay {
                        modes,
                        minute_of_day,
                    }),
                    7,
                )
            }
            0x76 => {
                let n = one()? as usize;
                checked_reply(reply, 0x76, 1 + n)?;
                let s = std::str::from_utf8(&d[1..1 + n])
                    .map_err(|_| Error::Protocol("invalid UTF-8 device name"))?;
                (Self::DeviceName(s.to_owned()), 1 + n)
            }
            0xb3 => (Self::LowPower(boolean(one()?)?), 1),
            0x2b => (Self::TemperatureUnit(unit(one()?)?), 1),
            0x2c => (
                Self::HourFormat(match one()? {
                    0 => HourFormat::Twelve,
                    1 => HourFormat::TwentyFour,
                    _ => return Err(Error::Protocol("invalid hour format")),
                }),
                1,
            ),
            0x83 => (Self::SongNameDisplay(boolean(one()?)?), 1),
            0x27 => {
                if boolean(one()?)? {
                    checked_reply(reply, 0x27, 5)?;
                    (Self::Password(Some([d[1], d[2], d[3], d[4]])), 5)
                } else {
                    (Self::Password(None), 1)
                }
            }
            0xbb => (
                Self::PowerOnVolume(
                    Percent::new(one()?).map_err(|_| Error::Protocol("invalid power-on volume"))?,
                ),
                1,
            ),
            0x8a => (Self::PowerOnChannel(PowerOnChannel::try_from(one()?)?), 1),
            0xac | 0x7d => {
                checked_reply(reply, reply.opcode, 2)?;
                (
                    if reply.opcode == 0xac {
                        Self::AutoPowerOff(le16(d))
                    } else {
                        Self::SdTrackCount(le16(d))
                    },
                    2,
                )
            }
            6 | 7 => {
                let prefix = if reply.opcode == 7 { 2 } else { 0 };
                checked_reply(reply, reply.opcode, prefix + 2)?;
                let n = if prefix == 2 {
                    le16(&d[prefix..])
                } else {
                    match name_length_order.ok_or(Error::Unsupported(
                        "select SD name length byte order explicitly",
                    ))? {
                        TransferLengthOrder::LittleEndian => le16(d),
                        TransferLengthOrder::BigEndian => u16::from_be_bytes([d[0], d[1]]),
                    }
                } as usize;
                if n > 128 && reply.opcode == 6 {
                    return Err(Error::Protocol("SD track name exceeds 128 bytes"));
                }
                checked_reply(reply, reply.opcode, prefix + 2 + n)?;
                (
                    Self::SdName(SdName {
                        music_id: if prefix == 2 { Some(le16(d)) } else { None },
                        unicode_bytes: d[prefix + 2..prefix + 2 + n].to_vec(),
                    }),
                    prefix + 2 + n,
                )
            }
            9 => (
                Self::Volume(Volume::new(one()?).map_err(|_| Error::Protocol("invalid volume"))?),
                1,
            ),
            11 => (Self::Playing(boolean(one()?)?), 1),
            0x14 => (Self::SdListComplete, 0),
            0xb4 => {
                checked_reply(reply, 0xb4, 9)?;
                (
                    Self::SdPlayback {
                        playback: SdPlayback {
                            position_seconds: le16(d),
                            music_id: le16(&d[4..]),
                            volume: Volume::new(d[7])
                                .map_err(|_| Error::Protocol("invalid SD volume"))?,
                            playing: boolean(d[6])?,
                            mode: PlayMode::try_from(d[8])?,
                        },
                        total_seconds: le16(&d[2..]),
                    },
                    9,
                )
            }
            0x47 => (Self::MusicListReady, 0),
            0xa8 => (Self::SoundControl(boolean(one()?)?), 1),
            0x11 => (Self::Acknowledged(0x11), 0),
            _ => return Err(Error::Unsupported("no documented system reply schema")),
        };
        Ok(Decoded {
            value,
            extension: d[used..].to_vec(),
        })
    }
}
fn unit(v: u8) -> Result<TemperatureUnit> {
    match v {
        0 => Ok(TemperatureUnit::Celsius),
        1 => Ok(TemperatureUnit::Fahrenheit),
        _ => Err(Error::Protocol("invalid temperature unit")),
    }
}

impl super::Command for SystemCommand {
    fn response_is_stream(&self) -> bool {
        matches!(self, Self::SdMusicList { .. })
    }
    fn response_is_notification(&self) -> bool {
        matches!(self, Self::GetWorkMode)
    }
    fn packet(&self) -> Result<Packet> {
        SystemCommand::packet(self)
    }
    fn response_opcode(&self) -> Option<u8> {
        SystemCommand::response_opcode(self)
    }
}
