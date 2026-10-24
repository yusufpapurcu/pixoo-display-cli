//! Pure Classic framing and the documented LE envelope.
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    opcode: u8,
    payload: Vec<u8>,
}
impl Packet {
    pub fn new(opcode: u8, payload: Vec<u8>) -> Result<Self> {
        if opcode < 4 || payload.len() > u16::MAX as usize - 3 {
            return Err(Error::Invalid(
                "opcode must be at least 4 and payload must fit a u16 packet",
            ));
        }
        Ok(Self { opcode, payload })
    }
    pub fn opcode(&self) -> u8 {
        self.opcode
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn reply(&self) -> Result<Reply> {
        if self.opcode != 4 || self.payload.len() < 2 {
            return Err(Error::Protocol("not a complete command reply"));
        }
        Ok(Reply {
            opcode: self.payload[0],
            status: self.payload[1],
            data: self.payload[2..].to_vec(),
        })
    }
    pub fn notification(&self) -> Result<Notification> {
        if self.opcode != 4 || self.payload.is_empty() {
            return Err(Error::Protocol("not a complete command notification"));
        }
        Ok(Notification {
            opcode: self.payload[0],
            data: self.payload[1..].to_vec(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub opcode: u8,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub opcode: u8,
    pub status: u8,
    pub data: Vec<u8>,
}
impl Reply {
    pub fn ensure_success(&self) -> Result<()> {
        if self.status == 0x55 {
            Ok(())
        } else {
            Err(Error::Rejected {
                opcode: self.opcode,
                status: self.status,
            })
        }
    }
    pub fn transport_ack(&self) -> Result<Option<u32>> {
        if self.opcode != 0x33 {
            return Ok(None);
        }
        self.ensure_success()?;
        let bytes: [u8; 4] = self
            .data
            .as_slice()
            .try_into()
            .map_err(|_| Error::Protocol("BLE ACK must contain a u32 packet number"))?;
        Ok(Some(u32::from_le_bytes(bytes)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassicDialect {
    /// Page146 framing, with the historical 03/04..06 escape convention.
    Escaped,
    /// Unescaped framing with a wrapping 16-bit sum, used by older displays.
    Unescaped,
    /// Captured Max behavior, widening sums >=0xffff to four bytes.
    PixooMax,
}

pub fn encode(packet: &Packet, dialect: ClassicDialect) -> Result<Vec<u8>> {
    let length =
        u16::try_from(packet.payload.len() + 3).map_err(|_| Error::Invalid("packet too large"))?;
    let mut body = Vec::with_capacity(length as usize + 4);
    body.extend_from_slice(&length.to_le_bytes());
    body.push(packet.opcode);
    body.extend_from_slice(&packet.payload);
    let sum: u32 = body.iter().map(|v| u32::from(*v)).sum();
    if dialect == ClassicDialect::PixooMax && sum >= 0xffff {
        body.extend_from_slice(&sum.to_le_bytes());
    } else {
        body.extend_from_slice(&(sum as u16).to_le_bytes());
    }
    let mut wire = Vec::with_capacity(body.len() + 2);
    wire.push(1);
    for b in body {
        if dialect == ClassicDialect::Escaped && (1..=3).contains(&b) {
            wire.extend_from_slice(&[3, b + 3]);
        } else {
            wire.push(b);
        }
    }
    wire.push(2);
    Ok(wire)
}

/// A strict bounded stream decoder. A corrupt frame requires an explicit reset.
pub struct Decoder {
    dialect: ClassicDialect,
    body: Vec<u8>,
    active: bool,
    escaped: bool,
    poisoned: bool,
    limit: usize,
}
impl Decoder {
    pub fn new(dialect: ClassicDialect) -> Self {
        Self::with_limit(dialect, u16::MAX as usize)
    }
    pub fn with_limit(dialect: ClassicDialect, maximum_body_length: usize) -> Self {
        Self {
            dialect,
            body: Vec::new(),
            active: false,
            escaped: false,
            poisoned: false,
            limit: maximum_body_length.min(u16::MAX as usize),
        }
    }
    pub fn reset(&mut self) {
        self.body.clear();
        self.active = false;
        self.escaped = false;
        self.poisoned = false;
    }
    pub fn is_empty(&self) -> bool {
        !self.active
    }
    pub fn finish(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Protocol("decoder is poisoned"))
        } else if self.active {
            Err(Error::Protocol("truncated frame"))
        } else {
            Ok(())
        }
    }
    pub fn feed(&mut self, input: &[u8]) -> Result<Vec<Packet>> {
        if self.poisoned {
            return Err(Error::Protocol("decoder requires reset"));
        }
        let result = self.feed_inner(input);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn feed_inner(&mut self, input: &[u8]) -> Result<Vec<Packet>> {
        let mut packets = Vec::new();
        for &raw in input {
            if !self.active {
                if raw != 1 {
                    return Err(Error::Protocol("invalid packet header"));
                }
                self.active = true;
                continue;
            }
            let mut complete_length = None;
            if self.body.len() >= 2 {
                let length = usize::from(u16::from_le_bytes([self.body[0], self.body[1]]));
                if length < 3 || length > self.limit {
                    return Err(Error::Protocol("invalid declared packet length"));
                }
                if self.body.len() >= length {
                    let sum: u32 = self.body[..length].iter().map(|v| u32::from(*v)).sum();
                    let width = if self.dialect == ClassicDialect::PixooMax && sum >= 0xffff {
                        4
                    } else {
                        2
                    };
                    if self.body.len() == length + width {
                        complete_length = Some((length, sum, width));
                    }
                }
            }
            if let Some((length, sum, width)) = complete_length {
                if self.escaped || raw != 2 {
                    return Err(Error::Protocol("invalid footer or oversized packet"));
                }
                let received = if width == 4 {
                    u32::from_le_bytes(
                        self.body[length..length + 4]
                            .try_into()
                            .map_err(|_| Error::Protocol("short checksum"))?,
                    )
                } else {
                    u32::from(u16::from_le_bytes([
                        self.body[length],
                        self.body[length + 1],
                    ]))
                };
                let expected = if width == 4 {
                    sum
                } else {
                    u32::from(sum as u16)
                };
                if received != expected {
                    return Err(Error::Protocol("checksum mismatch"));
                }
                packets.push(Packet::new(self.body[2], self.body[3..length].to_vec())?);
                self.body.clear();
                self.active = false;
                continue;
            }
            let value = if self.dialect == ClassicDialect::Escaped {
                if self.escaped {
                    self.escaped = false;
                    if !(4..=6).contains(&raw) {
                        return Err(Error::Protocol("invalid escape substitution"));
                    }
                    raw - 3
                } else if raw == 3 {
                    self.escaped = true;
                    continue;
                } else if raw == 1 || raw == 2 {
                    return Err(Error::Protocol("unescaped reserved byte"));
                } else {
                    raw
                }
            } else {
                raw
            };
            self.body.push(value);
            if self.body.len() > self.limit.saturating_add(4) {
                return Err(Error::Protocol("decoder size limit exceeded"));
            }
            if self.body.len() == 2 {
                let len = usize::from(u16::from_le_bytes([self.body[0], self.body[1]]));
                if len < 3 || len > self.limit {
                    return Err(Error::Protocol("invalid declared packet length"));
                }
            }
        }
        Ok(packets)
    }
}

/// Page146 excludes the identifier from length. Captured implementations include it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BleLength {
    Documented,
    IdentifierIncluded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BleEnvelope {
    pub packet_number: Option<u32>,
    pub data: Vec<u8>,
}
impl BleEnvelope {
    pub fn command(packet: &Packet, packet_number: Option<u32>) -> Self {
        let mut data = vec![packet.opcode];
        data.extend_from_slice(&packet.payload);
        Self {
            packet_number,
            data,
        }
    }
    pub fn encode(&self, rule: BleLength) -> Result<Vec<u8>> {
        let length = self
            .data
            .len()
            .checked_add(if self.packet_number.is_some() { 6 } else { 2 })
            .and_then(|n| n.checked_add(usize::from(rule == BleLength::IdentifierIncluded)))
            .and_then(|n| u16::try_from(n).ok())
            .ok_or(Error::Invalid("BLE envelope too large"))?;
        let mut wire = vec![0xfe, 0xef, 0xaa, 0x55];
        wire.extend_from_slice(&length.to_le_bytes());
        wire.push(u8::from(self.packet_number.is_some()));
        if let Some(id) = self.packet_number {
            wire.extend_from_slice(&id.to_le_bytes());
        }
        wire.extend_from_slice(&self.data);
        let sum = wire[4..]
            .iter()
            .fold(0u16, |acc, b| acc.wrapping_add(u16::from(*b)));
        wire.extend_from_slice(&sum.to_le_bytes());
        Ok(wire)
    }
}

pub struct BleDecoder {
    rule: BleLength,
    buffer: Vec<u8>,
    poisoned: bool,
}
impl BleDecoder {
    pub fn new(rule: BleLength) -> Self {
        Self {
            rule,
            buffer: Vec::new(),
            poisoned: false,
        }
    }
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.poisoned = false;
    }
    pub fn finish(&self) -> Result<()> {
        if self.poisoned || !self.buffer.is_empty() {
            Err(Error::Protocol("incomplete or corrupt BLE envelope"))
        } else {
            Ok(())
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<BleEnvelope>> {
        if self.poisoned {
            return Err(Error::Protocol("BLE decoder requires reset"));
        }
        let result = self.feed_inner(bytes);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn feed_inner(&mut self, bytes: &[u8]) -> Result<Vec<BleEnvelope>> {
        let mut result = Vec::new();
        for &b in bytes {
            self.buffer.push(b);
            let n = self.buffer.len();
            if n <= 4 && self.buffer[n - 1] != [0xfe, 0xef, 0xaa, 0x55][n - 1] {
                return Err(Error::Protocol("invalid BLE header"));
            }
            if n < 7 {
                continue;
            }
            let length = usize::from(u16::from_le_bytes([self.buffer[4], self.buffer[5]]));
            let extra = usize::from(self.rule == BleLength::Documented);
            let id = self.buffer[6];
            if id > 1 || length + extra < 3 + if id == 1 { 4 } else { 0 } {
                return Err(Error::Protocol("invalid BLE length or identifier"));
            }
            let total = length + 6 + extra;
            if n < total {
                continue;
            }
            if n != total {
                return Err(Error::Protocol("BLE envelope overrun"));
            }
            let sum = self.buffer[4..n - 2]
                .iter()
                .fold(0u16, |s, v| s.wrapping_add(u16::from(*v)));
            if sum != u16::from_le_bytes([self.buffer[n - 2], self.buffer[n - 1]]) {
                return Err(Error::Protocol("BLE checksum mismatch"));
            }
            let packet_number = if id == 1 {
                Some(u32::from_le_bytes(
                    self.buffer[7..11]
                        .try_into()
                        .map_err(|_| Error::Protocol("short BLE packet number"))?,
                ))
            } else {
                None
            };
            result.push(BleEnvelope {
                packet_number,
                data: self.buffer[if id == 1 { 11 } else { 7 }..n - 2].to_vec(),
            });
            self.buffer.clear();
        }
        Ok(result)
    }
}
