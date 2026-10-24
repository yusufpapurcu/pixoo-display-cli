//! Checked payload builders, organized by the official Bluetooth categories.
pub mod display;
pub mod schedule;
pub mod system;
pub mod tools;

use crate::{protocol::Packet, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percent(u8);
impl Percent {
    pub fn new(value: u8) -> Result<Self> {
        if value > 100 {
            Err(Error::Invalid("percentage must be 0..=100"))
        } else {
            Ok(Self(value))
        }
    }
    pub fn get(self) -> u8 {
        self.0
    }
}
impl TryFrom<u8> for Percent {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        Self::new(v)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub fn bytes(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTime {
    hour: u8,
    minute: u8,
}
impl ClockTime {
    pub fn new(hour: u8, minute: u8) -> Result<Self> {
        if hour > 23 || minute > 59 {
            Err(Error::Invalid("clock time must be 00:00..23:59"))
        } else {
            Ok(Self { hour, minute })
        }
    }
    pub fn hour(self) -> u8 {
        self.hour
    }
    pub fn minute(self) -> u8 {
        self.minute
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weekdays(u8);
impl Weekdays {
    pub fn new(mask: u8) -> Result<Self> {
        if mask & 0x80 != 0 {
            Err(Error::Invalid("weekday bits are Sunday0 through Saturday6"))
        } else {
            Ok(Self(mask))
        }
    }
    pub fn get(self) -> u8 {
        self.0
    }
}

/// Implemented by the domain command enums. Application replies are distinct from BLE ACKs.
pub trait Command {
    fn packet(&self) -> Result<Packet>;
    fn response_opcode(&self) -> Option<u8>;
    fn response_is_notification(&self) -> bool {
        false
    }
    fn response_is_stream(&self) -> bool {
        false
    }
}

/// Consumer-facing saved tab numbers. The Max has three independently stored tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredTab(u8);
impl StoredTab {
    pub fn new(one_based: u8) -> Result<Self> {
        if !(1..=3).contains(&one_based) {
            Err(Error::Invalid("stored tab must be 1..=3"))
        } else {
            Ok(Self(one_based))
        }
    }
    pub fn number(self) -> u8 {
        self.0
    }
    pub fn wire_index(self) -> u8 {
        self.0 - 1
    }
}

/// Additional captured Max selector, outside the 91-page official catalogue.
pub fn select_stored_tab(tab: StoredTab) -> Result<[Packet; 2]> {
    Ok([
        Packet::new(0x45, vec![5])?,
        Packet::new(0xbd, vec![0x17, tab.wire_index()])?,
    ])
}

/// Clock synchronization from the Go reference. Opcode18 is outside the official page set.
pub fn date_time(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> Result<Packet> {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if !(2000..=2099).contains(&year)
        || day == 0
        || day > days
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(Error::Invalid("invalid device calendar date"));
    }
    Packet::new(
        0x18,
        vec![
            (year % 100) as u8,
            (year / 100) as u8,
            month,
            day,
            hour,
            minute,
            second,
        ],
    )
}
