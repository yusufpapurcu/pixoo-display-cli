use super::{BluetoothAddress, RfcommChannel, Transport};
use std::{
    io, mem,
    os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket},
    sync::OnceLock,
    time::Duration,
};
use windows_sys::Win32::{
    Devices::Bluetooth::{AF_BTH, BTHPROTO_RFCOMM, SOCKADDR_BTH},
    Networking::WinSock::*,
};

pub struct Rfcomm {
    socket: OwnedSocket,
}

fn last_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}
fn startup() -> io::Result<()> {
    static STARTUP: OnceLock<i32> = OnceLock::new();
    let code = *STARTUP.get_or_init(|| {
        let mut data: WSADATA = unsafe { mem::zeroed() };
        unsafe { WSAStartup(0x202, &mut data) }
    });
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}
fn wait(socket: SOCKET, write: bool, timeout: Duration) -> io::Result<()> {
    let mut set: FD_SET = unsafe { mem::zeroed() };
    set.fd_count = 1;
    set.fd_array[0] = socket;
    let mut exceptions = set;
    let t = TIMEVAL {
        tv_sec: timeout.as_secs().min(i32::MAX as u64) as i32,
        tv_usec: timeout.subsec_micros() as i32,
    };
    let result = unsafe {
        select(
            0,
            if write {
                std::ptr::null_mut()
            } else {
                &mut set
            },
            if write {
                &mut set
            } else {
                std::ptr::null_mut()
            },
            if write {
                &mut exceptions
            } else {
                std::ptr::null_mut()
            },
            &t,
        )
    };
    if result == SOCKET_ERROR {
        return Err(last_error());
    }
    if result == 0 {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(())
}
impl Rfcomm {
    /// Connects a paired Classic device with a bounded nonblocking Winsock connect.
    pub fn connect(
        address: BluetoothAddress,
        channel: RfcommChannel,
        timeout: Duration,
    ) -> io::Result<Self> {
        startup()?;
        let raw = unsafe { socket(AF_BTH as i32, SOCK_STREAM, BTHPROTO_RFCOMM as i32) };
        if raw == INVALID_SOCKET {
            return Err(last_error());
        }
        let socket = unsafe { OwnedSocket::from_raw_socket(raw as _) };
        let mut nonblocking = 1u32;
        if unsafe { ioctlsocket(raw, FIONBIO, &mut nonblocking) } == SOCKET_ERROR {
            return Err(last_error());
        }
        let target = SOCKADDR_BTH {
            addressFamily: AF_BTH,
            btAddr: address.integer(),
            serviceClassId: unsafe { mem::zeroed() },
            port: u32::from(channel.get()),
        };
        let result = unsafe {
            connect(
                raw,
                &target as *const _ as *const SOCKADDR,
                mem::size_of::<SOCKADDR_BTH>() as i32,
            )
        };
        if result == SOCKET_ERROR {
            let error = unsafe { WSAGetLastError() };
            if error != WSAEWOULDBLOCK && error != WSAEINPROGRESS {
                return Err(io::Error::from_raw_os_error(error));
            }
            wait(raw, true, timeout)?;
            let mut code = 0i32;
            let mut len = mem::size_of::<i32>() as i32;
            if unsafe {
                getsockopt(
                    raw,
                    SOL_SOCKET,
                    SO_ERROR,
                    &mut code as *mut _ as *mut u8,
                    &mut len,
                )
            } == SOCKET_ERROR
            {
                return Err(last_error());
            }
            if code != 0 {
                return Err(io::Error::from_raw_os_error(code));
            }
        }
        Ok(Self { socket })
    }
}
impl Transport for Rfcomm {
    fn read(&mut self, bytes: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let raw = self.socket.as_raw_socket() as SOCKET;
        wait(raw, false, timeout)?;
        let len = bytes.len().min(i32::MAX as usize) as i32;
        let n = unsafe { recv(raw, bytes.as_mut_ptr(), len, 0) };
        if n == SOCKET_ERROR {
            Err(last_error())
        } else {
            Ok(n as usize)
        }
    }
    fn write(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let raw = self.socket.as_raw_socket() as SOCKET;
        wait(raw, true, timeout)?;
        let n = unsafe {
            send(
                raw,
                bytes.as_ptr(),
                bytes.len().min(i32::MAX as usize) as i32,
                0,
            )
        };
        if n == SOCKET_ERROR {
            Err(last_error())
        } else {
            Ok(n as usize)
        }
    }
    fn close(&mut self) -> io::Result<()> {
        if unsafe { shutdown(self.socket.as_raw_socket() as SOCKET, SD_BOTH) } == SOCKET_ERROR {
            Err(last_error())
        } else {
            Ok(())
        }
    }
}
