use super::{BluetoothAddress, RfcommChannel, Transport};
use std::{
    io, mem,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Duration,
};

#[repr(C)]
struct SockaddrRc {
    family: libc::sa_family_t,
    address: [u8; 6],
    channel: u8,
}
pub struct Rfcomm {
    socket: OwnedFd,
}
fn ready(fd: i32, write: bool, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .ok_or(io::ErrorKind::InvalidInput)?;
    loop {
        let mut p = libc::pollfd {
            fd,
            events: if write { libc::POLLOUT } else { libc::POLLIN },
            revents: 0,
        };
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let ms = left
            .as_millis()
            .saturating_add(u128::from(!left.is_zero()))
            .min(i32::MAX as u128) as i32;
        let n = unsafe { libc::poll(&mut p, 1, ms) };
        if n > 0 {
            return Ok(());
        }
        if n == 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}
impl Rfcomm {
    pub fn connect(
        address: BluetoothAddress,
        channel: RfcommChannel,
        timeout: Duration,
    ) -> io::Result<Self> {
        let fd = unsafe {
            libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                3,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let socket = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut bytes = address.bytes();
        bytes.reverse();
        let target = SockaddrRc {
            family: libc::AF_BLUETOOTH as libc::sa_family_t,
            address: bytes,
            channel: channel.get(),
        };
        let n = unsafe {
            libc::connect(
                fd,
                &target as *const _ as *const libc::sockaddr,
                mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EINPROGRESS) {
                return Err(e);
            }
            ready(fd, true, timeout)?;
            let mut code = 0i32;
            let mut len = mem::size_of::<i32>() as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    &mut code as *mut _ as *mut _,
                    &mut len,
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
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
        ready(self.socket.as_raw_fd(), false, timeout)?;
        let n = unsafe {
            libc::recv(
                self.socket.as_raw_fd(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
            )
        };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
    fn write(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        ready(self.socket.as_raw_fd(), true, timeout)?;
        let n = unsafe {
            libc::send(
                self.socket.as_raw_fd(),
                bytes.as_ptr().cast(),
                bytes.len(),
                libc::MSG_NOSIGNAL,
            )
        };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
    fn close(&mut self) -> io::Result<()> {
        if unsafe { libc::shutdown(self.socket.as_raw_fd(), libc::SHUT_RDWR) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
