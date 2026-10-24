//! Deadline-bounded byte transports. BLE transports carry notifications as bytes.
use crate::{Error, Result};
use std::{fmt, io, str::FromStr, time::Duration};

#[cfg(feature = "ble")]
pub mod ble;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
pub use linux::Rfcomm;
#[cfg(windows)]
pub use windows::Rfcomm;

/// Implementations must return by the timeout. Zero means a nonblocking attempt.
/// `Ok(0)` means EOF for a nonempty read and zero progress for a nonempty write.
/// Retriable write errors (`TimedOut`, `WouldBlock`, `Interrupted`) mean no bytes
/// were transferred. An ambiguous GATT write must report a nonretriable error kind.
pub trait Transport {
    fn read(&mut self, bytes: &mut [u8], timeout: Duration) -> io::Result<usize>;
    fn write(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<usize>;
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BluetoothAddress([u8; 6]);
impl BluetoothAddress {
    pub fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
    pub fn bytes(self) -> [u8; 6] {
        self.0
    }
    #[cfg(windows)]
    fn integer(self) -> u64 {
        self.0.into_iter().fold(0, |n, b| (n << 8) | u64::from(b))
    }
}
impl FromStr for BluetoothAddress {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let mut result = [0; 6];
        let parts: Vec<_> = value.split(':').collect();
        if parts.len() != 6 {
            return Err(Error::Invalid(
                "Bluetooth address needs six colon-separated hex bytes",
            ));
        }
        for (dest, part) in result.iter_mut().zip(parts) {
            if part.len() != 2 {
                return Err(Error::Invalid(
                    "Bluetooth address bytes need two hex digits",
                ));
            }
            *dest = u8::from_str_radix(part, 16)
                .map_err(|_| Error::Invalid("invalid Bluetooth address"))?;
        }
        Ok(Self(result))
    }
}
impl fmt::Display for BluetoothAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfcommChannel(u8);
impl RfcommChannel {
    pub fn new(channel: u8) -> Result<Self> {
        if !(1..=30).contains(&channel) {
            Err(Error::Invalid("RFCOMM channel must be 1..=30"))
        } else {
            Ok(Self(channel))
        }
    }
    pub fn get(self) -> u8 {
        self.0
    }
}
impl Default for RfcommChannel {
    fn default() -> Self {
        Self(1)
    }
}

/// For an explicitly configured Bluetooth proxy. This does not scan or pair devices.
pub struct TcpTransport(std::net::TcpStream);
impl TcpTransport {
    pub fn new(stream: std::net::TcpStream) -> Self {
        Self(stream)
    }
}
impl Transport for TcpTransport {
    fn read(&mut self, bytes: &mut [u8], timeout: Duration) -> io::Result<usize> {
        use io::Read;
        self.0.set_nonblocking(timeout.is_zero())?;
        self.0.set_read_timeout(if timeout.is_zero() {
            None
        } else {
            Some(timeout)
        })?;
        self.0.read(bytes)
    }
    fn write(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<usize> {
        use io::Write;
        self.0.set_nonblocking(timeout.is_zero())?;
        self.0.set_write_timeout(if timeout.is_zero() {
            None
        } else {
            Some(timeout)
        })?;
        self.0.write(bytes)
    }
    fn close(&mut self) -> io::Result<()> {
        self.0.shutdown(std::net::Shutdown::Both)
    }
}
