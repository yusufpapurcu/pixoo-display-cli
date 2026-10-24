use crate::{
    cli::{Fit, TextStyle},
    error::{CliError, Result},
};
use divoom_bluetooth::transport::{BluetoothAddress, RfcommChannel};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub default_device: Option<String>,
    #[serde(default)]
    pub devices: BTreeMap<String, Device>,
    #[serde(default)]
    pub scenes: BTreeMap<String, Scene>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            default_device: None,
            devices: BTreeMap::new(),
            scenes: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub address: String,
    pub channel: u8,
    pub profile: String,
    pub size: u32,
    pub controller: Option<String>,
}

#[derive(Deserialize)]
pub struct GoConfiguration {
    pub transport: String,
    pub mac: String,
    #[serde(default = "default_channel")]
    pub channel: u8,
    #[serde(rename = "listenAddr", default = "default_controller")]
    pub listen_addr: String,
}

fn default_channel() -> u8 {
    1
}

fn default_controller() -> String {
    ":8377".to_owned()
}
impl Device {
    pub fn validate(&mut self) -> Result<()> {
        self.address = self.address.parse::<BluetoothAddress>()?.to_string();
        RfcommChannel::new(self.channel)?;
        if self.profile != "pixoo-max" || self.size != 32 {
            return Err(CliError::unsupported(
                "this CLI supports pixoo-max at 32 pixels; other profiles remain library APIs",
            ));
        }
        if let Some(endpoint) = &self.controller {
            controller_address(endpoint)?;
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Scene {
    pub brightness: Option<u8>,
    #[serde(flatten)]
    pub source: SceneSource,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SceneSource {
    Tab { tab: u8 },
    File { file: PathBuf, fit: Fit },
    Text { text: String, style: TextStyle },
}

pub struct Store {
    pub path: PathBuf,
}
impl Store {
    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let path = match path {
            Some(path) => path,
            None => default_config()?,
        };
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        Ok(Self { path })
    }
    pub fn load(&self) -> Result<Config> {
        let bytes = match fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e.into()),
        };
        if bytes.len() > 1024 * 1024 {
            return Err(CliError::invalid("configuration exceeds 1 MiB"));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| CliError::invalid("configuration is not UTF-8"))?;
        let config: Config = toml::from_str(text)
            .map_err(|e| CliError::invalid(format!("invalid configuration: {e}")))?;
        if config.version != 1 {
            return Err(CliError::invalid("unsupported configuration version"));
        }
        Ok(config)
    }
    pub fn change<T>(&self, f: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CliError::invalid("config path needs a parent"))?;
        fs::create_dir_all(parent)?;
        let _lease = Lease::acquire(&self.path.with_extension("toml.lock"))?;
        let mut config = self.load()?;
        let result = f(&mut config)?;
        let encoded =
            toml::to_string_pretty(&config).map_err(|e| CliError::invalid(e.to_string()))?;
        let temp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let save = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            use std::io::Write;
            file.write_all(encoded.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if save.is_err() {
            let _ = fs::remove_file(&temp);
        }
        save?;
        Ok(result)
    }
    pub fn device(&self, selected: Option<&str>) -> Result<(String, Device)> {
        let config = self.load()?;
        let alias = selected
            .or(config.default_device.as_deref())
            .ok_or_else(|| {
                CliError::invalid(
                    "select a device with --device, PIXO_DEVICE, or 'pixo device use NAME'",
                )
            })?;
        let mut device = config
            .devices
            .get(alias)
            .cloned()
            .ok_or_else(|| CliError::invalid(format!("unknown device '{alias}'")))?;
        device.validate()?;
        Ok((alias.to_owned(), device))
    }
    pub fn scene(&self, name: &str) -> Result<Scene> {
        let mut scene = self
            .load()?
            .scenes
            .get(name)
            .cloned()
            .ok_or_else(|| CliError::invalid(format!("unknown scene '{name}'")))?;
        if let SceneSource::File { file, .. } = &mut scene.source {
            if file.is_relative() {
                *file = self.path.parent().unwrap().join(&*file);
            }
        }
        Ok(scene)
    }
}

pub struct Lease(File);
impl Lease {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
            {
                CliError::new(
                    "busy",
                    3,
                    "another process owns this configuration or device",
                )
            } else {
                CliError::from(e)
            }
        })?;
        Ok(Self(file))
    }
    pub fn device(device: &Device) -> Result<Self> {
        let address: BluetoothAddress = device.address.parse()?;
        let key = address.to_string().replace(':', "");
        Self::acquire(
            &state_dir()?
                .join("locks")
                .join(format!("classic-{key}.lock")),
        )
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(CliError::invalid(
            "names must be 1..64 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}
pub fn percent(value: Option<u8>) -> Result<Option<u8>> {
    if value.is_some_and(|v| v > 100) {
        return Err(CliError::invalid("brightness must be 0..100"));
    }
    Ok(value)
}
pub fn controller_address(endpoint: &str) -> Result<SocketAddr> {
    let endpoint = if endpoint.starts_with(':') {
        format!("127.0.0.1{endpoint}")
    } else {
        endpoint.replace("localhost:", "127.0.0.1:")
    };
    let mut address: SocketAddr = endpoint.parse().map_err(|_| {
        CliError::invalid("controller must be a local IP:port, localhost:port or :port")
    })?;
    if address.ip().is_unspecified() {
        address.set_ip(if address.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        });
    }
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(CliError::invalid(
            "controller endpoint must use a local loopback address and nonzero port",
        ));
    }
    Ok(address)
}
fn default_config() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("pixo/config.toml"));
    }
    #[cfg(windows)]
    if let Some(path) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(path).join("pixo/config.toml"));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            CliError::invalid("cannot find user configuration directory; use --config")
        })?;
    Ok(PathBuf::from(home).join(".config/pixo/config.toml"))
}
fn state_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("PIXO_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    #[cfg(windows)]
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(path).join("pixo"));
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("pixo"));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| CliError::invalid("cannot find user state directory"))?;
    Ok(PathBuf::from(home).join(".local/state/pixo"))
}
