use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "pixo",
    version,
    about = "Pixoo-Max artwork, text and named scenes over Classic Bluetooth",
    after_help = "One connection per invocation. No automatic save replay. Help, previews and dry runs never connect."
)]
pub struct Cli {
    #[arg(long, env = "PIXO_CONFIG", global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, env = "PIXO_DEVICE", global = true)]
    pub device: Option<String>,
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub verbose: bool,
    /// Total device budget, including connection and priming (default 10s controls, 60s media)
    #[arg(long, global=true, value_parser=parse_duration)]
    pub timeout: Option<std::time::Duration>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read bounded display status, preserving raw mode and extension bytes
    Status,
    /// Upload a transient image/GIF, or render a local preview
    Show {
        file: PathBuf,
        #[command(flatten)]
        options: MediaOptions,
    },
    /// Render printable ASCII text with the bundled 8x8 pixel font
    Text {
        #[command(flatten)]
        input: TextInput,
        #[command(flatten)]
        style: TextStyle,
        #[command(flatten)]
        options: OutputOptions,
    },
    /// Replace artwork in a device tab (requires --replace except in a dry run)
    Save {
        file: PathBuf,
        #[arg(long, value_parser=tab)]
        tab: u8,
        #[arg(long)]
        replace: bool,
        #[command(flatten)]
        options: MediaOptions,
    },
    /// Recall tab 1, 2 or 3 without uploading artwork
    Stored {
        #[arg(value_parser=tab)]
        tab: u8,
        #[arg(long)]
        brightness: Option<u8>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Read or set system brightness (0..100), checking the observed result
    Brightness {
        value: Option<u8>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Display a black frame; this does not power off the device
    Blank {
        #[command(flatten)]
        options: OutputOptions,
    },
    /// Read display mode or select custom/light mode
    Mode {
        #[command(subcommand)]
        mode: Option<ModeCommand>,
    },
    /// Manage local artwork recipes; edits do not change the device
    Scene {
        #[command(subcommand)]
        command: SceneCommand,
    },
    /// Manage exact paired-device configurations; no Classic scan or pairing
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    /// Check local configuration; --connect also performs a read-only status query
    Doctor {
        #[arg(long)]
        connect: bool,
    },
}

#[derive(Args, Debug, Clone)]
pub struct OutputOptions {
    /// Validate and describe the prepared operation without device or daemon I/O
    #[arg(long)]
    pub dry_run: bool,
    /// Write a local PNG (one frame) or GIF preview instead of connecting
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Set system brightness in the same connection before the display operation
    #[arg(long)]
    pub brightness: Option<u8>,
}

#[derive(Args, Debug, Clone)]
pub struct MediaOptions {
    #[arg(long, value_enum, default_value = "contain")]
    pub fit: Fit,
    #[command(flatten)]
    pub output: OutputOptions,
}

#[derive(ValueEnum, Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    #[default]
    Contain,
    Cover,
    Stretch,
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
pub struct TextInput {
    pub text: Option<String>,
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub stdin: bool,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextStyle {
    #[arg(long, default_value = "white")]
    pub color: String,
    #[arg(long, default_value = "black")]
    pub background: String,
    #[arg(long, value_enum, default_value = "center")]
    pub align: Align,
    #[arg(long, value_enum, default_value = "error", conflicts_with = "scroll")]
    pub overflow: Overflow,
    /// Alias for --overflow scroll
    #[arg(long)]
    pub scroll: bool,
    /// Duration per horizontal pixel step, such as 80ms
    #[arg(long, default_value="80ms", value_parser=parse_step)]
    pub step: u16,
    #[arg(long, default_value_t=0, value_parser=clap::value_parser!(u8).range(0..=8))]
    pub line_spacing: u8,
}

#[derive(ValueEnum, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Center,
    Right,
}
#[derive(ValueEnum, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    Error,
    Clip,
    Wrap,
    Scroll,
}

#[derive(Subcommand, Debug)]
pub enum ModeCommand {
    Custom {
        #[arg(long)]
        dry_run: bool,
    },
    Light {
        #[arg(long, default_value = "#FF5000")]
        color: String,
        #[arg(long, default_value_t = 40)]
        brightness: u8,
        #[arg(long, default_value_t = 0)]
        effect: u8,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SceneCommand {
    Add {
        name: String,
        #[arg(long, value_parser=tab, group="source")]
        tab: Option<u8>,
        #[arg(long, group = "source")]
        file: Option<PathBuf>,
        #[arg(long, group = "source")]
        text: Option<String>,
        #[arg(long, value_enum, default_value = "contain")]
        fit: Fit,
        #[arg(long)]
        brightness: Option<u8>,
        #[arg(long)]
        replace: bool,
        #[command(flatten)]
        style: TextStyle,
    },
    List,
    Show {
        name: String,
    },
    Remove {
        name: String,
    },
    Play {
        name: String,
        #[command(flatten)]
        options: OutputOptions,
    },
    /// Replace a device tab with a scene's encoded file/text; no implicit recall
    Upload {
        name: String,
        #[arg(long, value_parser=tab)]
        tab: u8,
        #[arg(long)]
        replace: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum DeviceCommand {
    Add {
        name: String,
        #[arg(long)]
        address: String,
        #[arg(long, default_value_t = 1)]
        channel: u8,
        #[arg(long, default_value = "pixoo-max")]
        profile: String,
        #[arg(long, default_value_t = 32)]
        size: u32,
        #[arg(long)]
        controller: Option<String>,
    },
    ImportGo {
        file: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        size: u32,
    },
    List,
    Use {
        name: String,
    },
    Capabilities,
}

fn tab(v: &str) -> std::result::Result<u8, String> {
    let n: u8 = v.parse().map_err(|_| "tab must be 1, 2 or 3")?;
    if (1..=3).contains(&n) {
        Ok(n)
    } else {
        Err("tab must be 1, 2 or 3".into())
    }
}
pub fn parse_duration(v: &str) -> std::result::Result<std::time::Duration, String> {
    let (n, multiplier) = if let Some(n) = v.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = v.strip_suffix('s') {
        (n, 1000)
    } else {
        (v, 1000)
    };
    let ms = n
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(multiplier))
        .filter(|n| *n > 0 && *n <= 3_600_000)
        .ok_or("duration must be 1ms..3600s")?;
    Ok(std::time::Duration::from_millis(ms))
}
fn parse_step(v: &str) -> std::result::Result<u16, String> {
    let ms = parse_duration(v)?.as_millis();
    u16::try_from(ms).map_err(|_| "step must be 1..65535ms".into())
}
