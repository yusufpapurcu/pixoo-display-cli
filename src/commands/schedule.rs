//! Alarm, memorial, time management and sleep scene commands.
//! FM frequency bytes and memorial title bytes retain their documented wire form.
use super::system::{
    animation_packets, boolean, checked_reply, valid_month_day, Decoded, TransferLengthOrder,
};
use super::{ClockTime, Percent, Rgb, Weekdays};
use crate::protocol::{Packet, Reply};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AlarmMode {
    Music = 0,
    Mode1 = 1,
    Mode2 = 2,
    Mode3 = 3,
    Mode4 = 4,
}
impl TryFrom<u8> for AlarmMode {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::Music,
            1 => Self::Mode1,
            2 => Self::Mode2,
            3 => Self::Mode3,
            4 => Self::Mode4,
            _ => return Err(Error::Protocol("unknown alarm mode")),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AlarmTrigger {
    Music = 1,
    Animation = 4,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alarm {
    pub index: u8,
    pub enabled: bool,
    pub time: ClockTime,
    pub weekdays: Weekdays,
    pub mode: AlarmMode,
    pub trigger: AlarmTrigger,
    pub fm_frequency: [u8; 2],
    pub volume: Percent,
}
impl Alarm {
    fn bytes(&self) -> Result<Vec<u8>> {
        if self.index >= 10 {
            return Err(Error::Invalid("alarm index must be 0..9"));
        }
        Ok(vec![
            self.index,
            u8::from(self.enabled),
            self.time.hour(),
            self.time.minute(),
            self.weekdays.get(),
            self.mode as u8,
            self.trigger as u8,
            self.fm_frequency[0],
            self.fm_frequency[1],
            self.volume.get(),
        ])
    }
    fn decode(d: &[u8]) -> Result<Self> {
        if d[0] >= 10 {
            return Err(Error::Protocol("invalid alarm index"));
        }
        Ok(Self {
            index: d[0],
            enabled: boolean(d[1])?,
            time: ClockTime::new(d[2], d[3]).map_err(|_| Error::Protocol("invalid alarm time"))?,
            weekdays: Weekdays::new(d[4]).map_err(|_| Error::Protocol("invalid alarm weekdays"))?,
            mode: AlarmMode::try_from(d[5])?,
            trigger: match d[6] {
                1 => AlarmTrigger::Music,
                4 => AlarmTrigger::Animation,
                _ => return Err(Error::Protocol("invalid alarm trigger")),
            },
            fm_frequency: [d[7], d[8]],
            volume: Percent::new(d[9]).map_err(|_| Error::Protocol("invalid alarm volume"))?,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memorial {
    pub index: u8,
    pub enabled: bool,
    pub month: u8,
    pub day: u8,
    pub time: ClockTime,
    pub has_animation: bool,
    pub title: [u8; 32],
}
impl Memorial {
    fn bytes(&self) -> Result<Vec<u8>> {
        if self.index > 2 {
            return Err(Error::Invalid("memorial setter index must be 0..2"));
        }
        if self.enabled {
            valid_month_day(self.month, self.day)?;
        }
        let mut b = vec![
            self.index,
            u8::from(self.enabled),
            self.month,
            self.day,
            self.time.hour(),
            self.time.minute(),
            u8::from(self.has_animation),
        ];
        b.extend_from_slice(&self.title);
        Ok(b)
    }
    fn decode(d: &[u8]) -> Result<Self> {
        if d[0] > 9 {
            return Err(Error::Protocol("invalid memorial reply index"));
        }
        let mut title = [0; 32];
        title.copy_from_slice(&d[7..39]);
        Ok(Self {
            index: d[0],
            enabled: boolean(d[1])?,
            month: d[2],
            day: d[3],
            time: ClockTime::new(d[4], d[5])
                .map_err(|_| Error::Protocol("invalid memorial time"))?,
            has_animation: boolean(d[6])?,
            title,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VoiceAlarmControl {
    Record = 0,
    Play = 1,
    Stop = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TimeManagementControl {
    Get = 0,
    Disable = 1,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeManagementRecord {
    pub total_records: u8,
    pub index: u8,
    pub start: ClockTime,
    pub end: ClockTime,
    /// Only sports mode uses this field. Other modes must supply zero.
    pub sports_minutes: u8,
    pub voice: bool,
    /// The official page assigns no meanings or range to display/cycle modes.
    pub display_mode: u8,
    pub cycle_mode: u8,
    pub picture: Vec<u8>,
}
impl TimeManagementRecord {
    fn bytes(&self) -> Result<Vec<u8>> {
        if self.total_records == 0 || self.total_records > 10 || self.index >= self.total_records {
            return Err(Error::Invalid(
                "time management record index/total is invalid",
            ));
        }
        let n = u16::try_from(self.picture.len())
            .map_err(|_| Error::Invalid("time management picture exceeds 65535 bytes"))?;
        let mut b = vec![
            self.total_records,
            self.index,
            self.start.hour(),
            self.start.minute(),
            self.end.hour(),
            self.end.minute(),
            self.sports_minutes,
            u8::from(self.voice),
            self.display_mode,
            self.cycle_mode,
        ];
        b.extend_from_slice(&n.to_le_bytes());
        b.extend_from_slice(&self.picture);
        Ok(b)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    pub minutes: u8,
    pub mode: u8,
    pub enabled: bool,
    pub fm_frequency: [u8; 2],
    /// No volume range is assigned to the scene structure in the source page.
    pub volume: u8,
    pub color: Rgb,
    pub light: u8,
}
impl Scene {
    fn bytes(&self) -> Vec<u8> {
        vec![
            self.minutes,
            self.mode,
            u8::from(self.enabled),
            self.fm_frequency[0],
            self.fm_frequency[1],
            self.volume,
            self.color.r,
            self.color.g,
            self.color.b,
            self.light,
        ]
    }
    fn decode(d: &[u8]) -> Result<Self> {
        Ok(Self {
            minutes: d[0],
            mode: d[1],
            enabled: boolean(d[2])?,
            fm_frequency: [d[3], d[4]],
            volume: d[5],
            color: Rgb {
                r: d[6],
                g: d[7],
                b: d[8],
            },
            light: d[9],
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleCommand {
    GetAlarms,
    Alarm(Alarm),
    Memorial(Memorial),
    GetMemorials,
    AlarmListen {
        enabled: bool,
        mode: u8,
        volume: Percent,
    },
    AlarmVolume(Percent),
    VoiceAlarm {
        control: VoiceAlarmControl,
        index: u8,
    },
    TimeManagement(TimeManagementRecord),
    TimeManagementControl(TimeManagementControl),
    GetSleepScene,
    SleepListen {
        enabled: bool,
        mode: u8,
        volume: u8,
    },
    SceneVolume(Percent),
    SleepColor(Rgb),
    SleepLight(u8),
    SleepAutoOff(Scene),
    SleepScene(Scene),
}
impl ScheduleCommand {
    pub fn response_opcode(&self) -> Option<u8> {
        match self {
            Self::GetAlarms => Some(0x42),
            Self::GetMemorials => Some(0x53),
            Self::TimeManagementControl(TimeManagementControl::Get) => Some(0x57),
            Self::GetSleepScene => Some(0xa2),
            Self::VoiceAlarm { .. } => Some(0x82),
            _ => None,
        }
    }
    pub fn packet(&self) -> Result<Packet> {
        let (op, data) = match self {
            Self::GetAlarms => (0x42, vec![]),
            Self::Alarm(v) => (0x43, v.bytes()?),
            Self::Memorial(v) => (0x54, v.bytes()?),
            Self::GetMemorials => (0x53, vec![]),
            Self::AlarmListen {
                enabled,
                mode,
                volume,
            } => (0xa5, vec![u8::from(*enabled), *mode, volume.get()]),
            Self::AlarmVolume(v) => (0xa6, vec![v.get()]),
            Self::VoiceAlarm { control, index } => {
                if *index >= 10 {
                    return Err(Error::Invalid("alarm index must be 0..9"));
                }
                (0x82, vec![*control as u8, *index])
            }
            Self::TimeManagement(v) => (0x56, v.bytes()?),
            Self::TimeManagementControl(v) => (0x57, vec![*v as u8]),
            Self::GetSleepScene => (0xa2, vec![]),
            Self::SleepListen {
                enabled,
                mode,
                volume,
            } => (0xa3, vec![u8::from(*enabled), *mode, *volume]),
            Self::SceneVolume(v) => (0xa4, vec![v.get()]),
            Self::SleepColor(v) => (0xad, vec![v.r, v.g, v.b]),
            Self::SleepLight(v) => (0xae, vec![*v]),
            Self::SleepAutoOff(v) => (0x40, v.bytes()),
            Self::SleepScene(v) => (0x41, v.bytes()),
        };
        Packet::new(op, data)
    }
}
pub fn alarm_animation_packets(index: u8, encoded: &[u8]) -> Result<Vec<Packet>> {
    if index >= 10 {
        return Err(Error::Invalid("alarm index must be 0..9"));
    }
    animation_packets(0x51, index, encoded, TransferLengthOrder::LittleEndian)
}
pub fn memorial_animation_packets(index: u8, encoded: &[u8]) -> Result<Vec<Packet>> {
    if index > 2 {
        return Err(Error::Invalid("memorial setter index must be 0..2"));
    }
    animation_packets(0x55, index, encoded, TransferLengthOrder::LittleEndian)
}

/// Page 246 references an unavailable EXT2 definition. Select the page-247
/// ten-byte layout explicitly when the device is known to use that layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmReplyLayout {
    Page247,
}
pub fn decode_alarms(reply: &Reply, layout: AlarmReplyLayout) -> Result<Decoded<Vec<Alarm>>> {
    let AlarmReplyLayout::Page247 = layout;
    let d = checked_reply(reply, 0x42, 100)?;
    let alarms = d[..100]
        .chunks_exact(10)
        .map(Alarm::decode)
        .collect::<Result<Vec<_>>>()?;
    Ok(Decoded {
        value: alarms,
        extension: d[100..].to_vec(),
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleReply {
    Memorials(Vec<Memorial>),
    TimeManagementConfigured(bool),
    SleepScene(Scene),
    VoiceAlarmAcknowledged,
}
impl ScheduleReply {
    pub fn decode(reply: &Reply) -> Result<Decoded<Self>> {
        reply.ensure_success()?;
        let (value, used) = match reply.opcode {
            0x53 => {
                let d = checked_reply(reply, 0x53, 390)?;
                (
                    Self::Memorials(
                        d[..390]
                            .chunks_exact(39)
                            .map(Memorial::decode)
                            .collect::<Result<Vec<_>>>()?,
                    ),
                    390,
                )
            }
            0x57 => {
                let d = checked_reply(reply, 0x57, 1)?;
                (Self::TimeManagementConfigured(boolean(d[0])?), 1)
            }
            0xa2 => {
                let d = checked_reply(reply, 0xa2, 10)?;
                (Self::SleepScene(Scene::decode(d)?), 10)
            }
            0x82 => (Self::VoiceAlarmAcknowledged, 0),
            _ => return Err(Error::Unsupported("no documented schedule reply schema")),
        };
        Ok(Decoded {
            value,
            extension: reply.data[used..].to_vec(),
        })
    }
}
impl super::Command for ScheduleCommand {
    fn packet(&self) -> Result<Packet> {
        ScheduleCommand::packet(self)
    }
    fn response_opcode(&self) -> Option<u8> {
        ScheduleCommand::response_opcode(self)
    }
}
