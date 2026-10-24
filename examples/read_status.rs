//! Read-only native transport instrument. Reads the existing paired-device config.
#[cfg(any(windows, target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use divoom_bluetooth::{
        commands::display::LightSettings,
        profile::DeviceProfile,
        session::{CancellationToken, Session, SessionOptions},
        transport::{BluetoothAddress, Rfcomm, RfcommChannel},
    };
    use std::{
        net::{SocketAddr, TcpStream},
        time::Duration,
    };
    let path = std::env::args()
        .nth(1)
        .ok_or("provide the path to the existing Go config.json")?;
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    if config["transport"].as_str() != Some("rfcomm") {
        return Err("this instrument requires an RFCOMM configuration".into());
    }
    let listen = config["listenAddr"].as_str().unwrap_or(":8377");
    let listen = if listen.starts_with(':') {
        format!("127.0.0.1{listen}")
    } else {
        listen.to_owned()
    };
    let mut daemon: SocketAddr = listen.parse()?;
    if daemon.ip().is_unspecified() {
        daemon.set_ip(if daemon.is_ipv4() {
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        } else {
            std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
        });
    }
    if TcpStream::connect_timeout(&daemon, Duration::from_millis(300)).is_ok() {
        return Err(
            "the Go controller is listening; release its connection before this instrument".into(),
        );
    }
    let address: BluetoothAddress = config["mac"]
        .as_str()
        .ok_or("missing configured Bluetooth address")?
        .parse()?;
    let channel = RfcommChannel::new(u8::try_from(config["channel"].as_u64().unwrap_or(1))?)?;
    let link = Rfcomm::connect(address, channel, Duration::from_secs(5))?;
    let mut session = Session::new(link, DeviceProfile::pixoo_max(), SessionOptions::default())?;
    let reply = session.prime(3, Duration::from_secs(1), &CancellationToken::default())?;
    println!("{:?}", LightSettings::decode(&reply)?);
    session.close()?;
    Ok(())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("This read-only instrument requires the Windows or Linux native Classic backend.");
}
