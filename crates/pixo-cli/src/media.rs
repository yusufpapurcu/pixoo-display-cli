use crate::{
    cli::{Align, Fit, Overflow, TextStyle},
    error::{CliError, Result},
};
use divoom_bluetooth::media::{Alignment, Animation, Dimension, FitMode, Frame, TextBitmap};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use image::{ImageReader, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
};

const MAX_FILE: u64 = 16 * 1024 * 1024;
pub struct Asset {
    pub animation: Animation,
    pub native: Vec<u8>,
    pub description: Value,
}
impl Asset {
    pub fn new(animation: Animation) -> Result<Self> {
        let frames = animation
            .frames()
            .iter()
            .map(|frame| {
                let mut pixels = frame.image().clone();
                for p in pixels.pixels_mut() {
                    *p = if p[3] <= 32 {
                        Rgba([0, 0, 0, 255])
                    } else {
                        Rgba([p[0], p[1], p[2], 255])
                    };
                }
                Frame::new(pixels, frame.duration_ms())
            })
            .collect::<divoom_bluetooth::Result<Vec<_>>>()?;
        let animation = Animation::new(frames)?;
        let native = animation.encode_native()?;
        if native.len() > 256 * 65536 {
            return Err(CliError::invalid("native artwork exceeds storage counters"));
        }
        let description = json!({"width":32,"height":32,"frames":animation.frames().len(),
            "durations_ms":animation.frames().iter().map(Frame::duration_ms).collect::<Vec<_>>(),
            "native_bytes":native.len(),"display_packets":if animation.frames().len()==1 {1}else {native.len().div_ceil(200)},"initial_storage_chunks":native.len().div_ceil(256),
            "gif_repetition":"not_preserved"});
        Ok(Self {
            animation,
            native,
            description,
        })
    }
    pub fn file(path: &Path, fit: Fit) -> Result<Self> {
        let bytes = read_bounded(path, MAX_FILE)?;
        let fit_name = serde_json::to_value(fit).expect("fit modes serialize");
        let fit = match fit {
            Fit::Contain => FitMode::Contain,
            Fit::Cover => FitMode::Cover,
            Fit::Stretch => FitMode::Stretch,
        };
        let (animation, source_width, source_height) =
            if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
                let animation =
                    Animation::from_gif_fitted(Cursor::new(&bytes), Dimension::ThirtyTwo, fit)?;
                (
                    animation,
                    u32::from(u16::from_le_bytes([bytes[6], bytes[7]])),
                    u32::from(u16::from_le_bytes([bytes[8], bytes[9]])),
                )
            } else {
                let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(4096);
                limits.max_image_height = Some(4096);
                limits.max_alloc = Some(64 * 1024 * 1024);
                reader.limits(limits);
                let image = reader
                    .decode()
                    .map_err(|e| CliError::invalid(format!("cannot decode image: {e}")))?;
                let animation = Animation::new(vec![Frame::from_image_fitted(
                    &image,
                    Dimension::ThirtyTwo,
                    0,
                    fit,
                )?])?;
                (animation, image.width(), image.height())
            };
        let mut asset = Self::new(animation)?;
        asset.description["source_width"] = json!(source_width);
        asset.description["source_height"] = json!(source_height);
        asset.description["fit"] = fit_name;
        Ok(asset)
    }
    pub fn blank() -> Result<Self> {
        Self::new(Animation::new(vec![Frame::new(
            RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255])),
            0,
        )?])?)
    }
    pub fn text(text: &str, style: &TextStyle) -> Result<Self> {
        if text.len() > 4096 {
            return Err(CliError::invalid("text exceeds 4096 UTF-8 bytes"));
        }
        let text = text.replace("\r\n", "\n");
        let text = text.trim_end_matches('\n');
        if text.is_empty() {
            return Err(CliError::invalid("text must not be empty"));
        }
        for c in text.chars() {
            if c != '\n' && (!(' '..='~').contains(&c) || BASIC_FONTS.get(c).is_none()) {
                return Err(CliError::invalid(format!(
                    "missing pixel-font glyph U+{:04X}; supported characters are printable ASCII",
                    c as u32
                )));
            }
        }
        let overflow = if style.scroll {
            Overflow::Scroll
        } else {
            style.overflow
        };
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if matches!(overflow, Overflow::Wrap) {
            lines = lines
                .into_iter()
                .flat_map(|line| {
                    let chars: Vec<_> = line.chars().collect();
                    if chars.is_empty() {
                        vec![String::new()]
                    } else {
                        chars.chunks(4).map(|c| c.iter().collect()).collect()
                    }
                })
                .collect();
        }
        let width = lines.iter().map(|l| l.len() * 8).max().unwrap_or(0);
        let height =
            lines.len() * (8 + usize::from(style.line_spacing)) - usize::from(style.line_spacing);
        if width == 0 || width > 4096 || height > 64 {
            return Err(CliError::invalid(
                "text bitmap must be 1..4096 pixels wide and at most 64 pixels high",
            ));
        }
        if matches!(overflow, Overflow::Error | Overflow::Wrap) && (width > 32 || height > 32) {
            return Err(CliError::invalid(format!("text needs {width}x{height} pixels; choose --overflow scroll, clip, or a shorter message")));
        }
        if matches!(overflow, Overflow::Scroll) && lines.len() != 1 {
            return Err(CliError::invalid(
                "horizontal scrolling requires a single text line",
            ));
        }
        let mut columns = vec![0u64; width];
        for (line_index, line) in lines.iter().enumerate() {
            let offset = match style.align {
                Align::Left => 0,
                Align::Center => (width - line.len() * 8) / 2,
                Align::Right => width - line.len() * 8,
            };
            let y = line_index * (8 + usize::from(style.line_spacing));
            for (letter, c) in line.chars().enumerate() {
                let glyph = BASIC_FONTS
                    .get(c)
                    .ok_or_else(|| CliError::invalid("missing glyph"))?;
                for (gy, row) in glyph.iter().enumerate() {
                    for x in 0..8 {
                        if row & (1 << x) != 0 {
                            columns[offset + letter * 8 + x] |= 1u64 << (y + gy);
                        }
                    }
                }
            }
        }
        let bitmap = TextBitmap::new(columns, height as u8)?;
        let align = match style.align {
            Align::Left => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::End,
        };
        let foreground = color(&style.color)?;
        let background = color(&style.background)?;
        let animation = if matches!(overflow, Overflow::Scroll) {
            if width + 33 > 4096 {
                return Err(CliError::invalid("scrolling text would exceed 4096 frames"));
            }
            bitmap.scrolling(
                Dimension::ThirtyTwo,
                Alignment::Center,
                foreground,
                background,
                style.step,
            )?
        } else {
            Animation::new(vec![bitmap.static_frame(
                Dimension::ThirtyTwo,
                align,
                Alignment::Center,
                foreground,
                background,
            )?])?
        };
        Self::new(animation)
    }
    pub fn preview(&self, path: &Path) -> Result<()> {
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(extension.as_deref(), Some("png" | "gif")) {
            return Err(CliError::invalid("preview output must end in .png or .gif"));
        }
        if extension.as_deref() == Some("png") && self.animation.frames().len() != 1 {
            return Err(CliError::invalid(
                "PNG holds one frame; use a .gif output for animations",
            ));
        }
        let mut destination = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let result = (|| -> Result<()> {
            match extension.as_deref() {
                Some("png") if self.animation.frames().len() == 1 => {
                    let mut writer = std::io::BufWriter::new(&mut destination);
                    self.animation.frames()[0]
                        .image()
                        .write_to(&mut writer, image::ImageFormat::Png)
                        .map_err(|e| CliError::new("local_io", 1, e.to_string()))?;
                    std::io::Write::flush(&mut writer)?;
                }
                Some("png") => {
                    return Err(CliError::invalid(
                        "PNG holds one frame; use a .gif output for animations",
                    ))
                }
                Some("gif") => {
                    let mut encoder = gif::Encoder::new(&mut destination, 32, 32, &[])
                        .map_err(|e| CliError::new("local_io", 1, e.to_string()))?;
                    encoder
                        .set_repeat(gif::Repeat::Infinite)
                        .map_err(|e| CliError::new("local_io", 1, e.to_string()))?;
                    for frame in self.animation.frames() {
                        let mut rgba = frame.image().as_raw().clone();
                        let mut output = gif::Frame::from_rgba_speed(32, 32, &mut rgba, 10);
                        output.delay = ((u32::from(frame.duration_ms()) + 5) / 10) as u16;
                        output.dispose = gif::DisposalMethod::Keep;
                        encoder
                            .write_frame(&output)
                            .map_err(|e| CliError::new("local_io", 1, e.to_string()))?;
                    }
                    encoder.into_inner()?;
                }
                _ => return Err(CliError::invalid("preview output must end in .png or .gif")),
            }
            destination.sync_all()?;
            Ok(())
        })();
        drop(destination);
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }
}

pub fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(CliError::invalid(format!("input exceeds {max} bytes")));
    }
    Ok(bytes)
}
pub fn color(value: &str) -> Result<[u8; 3]> {
    let value = value.to_ascii_lowercase();
    let named = match value.as_str() {
        "black" => Some([0, 0, 0]),
        "white" => Some([255, 255, 255]),
        "red" => Some([255, 0, 0]),
        "green" => Some([0, 255, 0]),
        "blue" => Some([0, 0, 255]),
        "yellow" => Some([255, 255, 0]),
        "orange" => Some([255, 80, 0]),
        "cyan" => Some([0, 255, 255]),
        "magenta" => Some([255, 0, 255]),
        _ => None,
    };
    if let Some(c) = named {
        return Ok(c);
    }
    let hex = value
        .strip_prefix('#')
        .filter(|s| s.len() == 6 && s.is_ascii())
        .ok_or_else(|| {
            CliError::invalid(
                "color must be #RRGGBB or black/white/red/green/blue/yellow/orange/cyan/magenta",
            )
        })?;
    let mut out = [0; 3];
    for (i, c) in out.iter_mut().enumerate() {
        *c = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| CliError::invalid("invalid hex color"))?;
    }
    Ok(out)
}
