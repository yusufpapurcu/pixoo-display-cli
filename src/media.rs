//! Native palette frames and Bluetooth image/animation commands.
//!
//! The 16/32 pixel format follows the independently licensed Go implementation
//! in `vendor/go-divoom`. Official commands 0x44 and 0x49 describe the transport,
//! but do not publish the native image encoder. A 64 pixel frame can be loaded
//! and resized here; its Bluetooth native encoding is deliberately unsupported.

use std::collections::HashMap;
use std::io::Read;

use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

use crate::protocol::Packet;
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Sixteen,
    ThirtyTwo,
    SixtyFour,
}

/// Aspect-ratio policy applied after a GIF frame has been fully composited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitMode {
    Stretch,
    Contain,
    Cover,
}

impl Dimension {
    pub const fn pixels(self) -> u32 {
        match self {
            Self::Sixteen => 16,
            Self::ThirtyTwo => 32,
            Self::SixtyFour => 64,
        }
    }

    fn of(image: &RgbaImage) -> Result<Self> {
        match image.dimensions() {
            (16, 16) => Ok(Self::Sixteen),
            (32, 32) => Ok(Self::ThirtyTwo),
            (64, 64) => Ok(Self::SixtyFour),
            _ => Err(Error::Invalid(
                "frame must be square with side 16, 32, or 64",
            )),
        }
    }
}

/// A full composited image and its display duration, in milliseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    image: RgbaImage,
    duration_ms: u16,
    dimension: Dimension,
}

impl Frame {
    pub fn new(image: RgbaImage, duration_ms: u16) -> Result<Self> {
        let dimension = Dimension::of(&image)?;
        Ok(Self {
            image,
            duration_ms,
            dimension,
        })
    }

    /// Stretch to a square using nearest-neighbor sampling, preserving pixel art.
    pub fn from_image(
        image: &DynamicImage,
        dimension: Dimension,
        duration_ms: u16,
    ) -> Result<Self> {
        Self::from_image_fitted(image, dimension, duration_ms, FitMode::Stretch)
    }

    /// Fit pixels with nearest-neighbor sampling. Contain pads with opaque black.
    /// Cover samples the centered source square directly into the output canvas.
    pub fn from_image_fitted(
        image: &DynamicImage,
        dimension: Dimension,
        duration_ms: u16,
        fit: FitMode,
    ) -> Result<Self> {
        if image.width() == 0 || image.height() == 0 {
            return Err(Error::Invalid("source image is empty"));
        }
        let size = dimension.pixels();
        let filter = image::imageops::FilterType::Nearest;
        let pixels = match fit {
            FitMode::Stretch => image.resize_exact(size, size, filter).to_rgba8(),
            FitMode::Cover => {
                let side = image.width().min(image.height());
                let denominator = u64::from(size) * 2;
                let offset_x = u64::from(image.width() - side) * u64::from(size);
                let offset_y = u64::from(image.height() - side) * u64::from(size);
                RgbaImage::from_fn(size, size, |x, y| {
                    let source_x =
                        ((u64::from(x) * 2 + 1) * u64::from(side) + offset_x) / denominator;
                    let source_y =
                        ((u64::from(y) * 2 + 1) * u64::from(side) + offset_y) / denominator;
                    image.get_pixel(source_x as u32, source_y as u32)
                })
            }
            FitMode::Contain => {
                let fitted = image.resize(size, size, filter).to_rgba8();
                let mut canvas = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 255]));
                image::imageops::replace(
                    &mut canvas,
                    &fitted,
                    i64::from((size - fitted.width()) / 2),
                    i64::from((size - fitted.height()) / 2),
                );
                canvas
            }
        };
        Self::new(pixels, duration_ms)
    }

    /// Load PNG, JPEG, BMP, WebP or a still GIF through the image decoder.
    /// Use `Animation::from_gif` when all GIF frames and disposal are required.
    pub fn from_bytes(bytes: &[u8], dimension: Dimension, duration_ms: u16) -> Result<Self> {
        let image =
            image::load_from_memory(bytes).map_err(|error| Error::Media(error.to_string()))?;
        Self::from_image(&image, dimension, duration_ms)
    }

    pub fn image(&self) -> &RgbaImage {
        &self.image
    }
    pub const fn duration_ms(&self) -> u16 {
        self.duration_ms
    }
    pub const fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// Encode one AA container, retaining this frame's duration.
    pub fn encode_native(&self) -> Result<Vec<u8>> {
        self.encode_with_duration(self.duration_ms)
    }

    fn encode_with_duration(&self, duration_ms: u16) -> Result<Vec<u8>> {
        let body = self.encode_body(duration_ms)?;
        wrap_frame(&body)
    }

    fn encode_body(&self, duration_ms: u16) -> Result<Vec<u8>> {
        if self.dimension == Dimension::SixtyFour {
            return Err(Error::Unsupported(
                "64 pixel native Bluetooth frame encoding is undocumented",
            ));
        }
        let mut palette = Vec::<[u8; 3]>::new();
        let mut indices = HashMap::new();
        let mut pixels = Vec::with_capacity(self.image.len() / 4);
        for p in self.image.pixels() {
            let color = if p[3] <= 32 {
                [0, 0, 0]
            } else {
                [p[0], p[1], p[2]]
            };
            let next =
                u16::try_from(palette.len()).map_err(|_| Error::Invalid("palette exceeds u16"))?;
            let index = *indices.entry(color).or_insert_with(|| {
                palette.push(color);
                next
            });
            pixels.push(index);
        }
        let count = if palette.len() == pixels.len() {
            0
        } else {
            palette.len()
        };
        let mut body = duration_ms.to_le_bytes().to_vec();
        match self.dimension {
            Dimension::Sixteen => body.extend_from_slice(&[
                0,
                u8::try_from(count)
                    .map_err(|_| Error::Invalid("16 pixel palette exceeds 256 colors"))?,
            ]),
            Dimension::ThirtyTwo => {
                body.push(3);
                body.extend_from_slice(
                    &u16::try_from(count)
                        .map_err(|_| Error::Invalid("palette exceeds u16"))?
                        .to_le_bytes(),
                );
            }
            Dimension::SixtyFour => unreachable!(),
        }
        for color in palette.iter() {
            body.extend_from_slice(color);
        }
        body.extend(pack_pixels(&pixels, palette.len())?);
        Ok(body)
    }

    /// Command 0x44 uses a zero duration, independent of this frame's timing.
    pub fn static_packet(&self) -> Result<Packet> {
        let mut payload = vec![0, 10, 10, 4];
        payload.extend(self.encode_with_duration(0)?);
        Packet::new(0x44, payload)
    }
}

/// The counter widths are a device property, independent of image resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaProfile {
    /// Official command 0x49, u16 total size and u8 chunk index.
    Standard,
    /// Pixoo Max, u32 total size and u16 chunk index, including 16 pixel content.
    PixooMax,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animation {
    frames: Vec<Frame>,
}

impl Animation {
    pub fn new(frames: Vec<Frame>) -> Result<Self> {
        let first = frames
            .first()
            .ok_or(Error::Invalid("animation needs at least one frame"))?;
        if frames
            .iter()
            .any(|frame| frame.dimension != first.dimension)
        {
            return Err(Error::Invalid(
                "animation frames must have a common resolution",
            ));
        }
        Ok(Self { frames })
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
    pub fn dimension(&self) -> Dimension {
        self.frames[0].dimension
    }

    /// Native stream for file storage or command 0x49. A single frame has time 0.
    /// Pixoo Max 32 pixel streams begin with the observed 05 and 06 flag frames.
    pub fn encode_native(&self) -> Result<Vec<u8>> {
        let mut stream = Vec::new();
        if self.dimension() == Dimension::ThirtyTwo {
            stream.extend_from_slice(&[0xaa, 8, 0, 0, 0, 5, 0, 0]);
            stream.extend_from_slice(&[0xaa, 9, 0, 0, 0, 6, 0, 0, 0]);
        }
        for frame in &self.frames {
            stream.extend(frame.encode_with_duration(if self.frames.len() == 1 {
                0
            } else {
                frame.duration_ms
            })?);
        }
        Ok(stream)
    }

    pub fn packets(&self, profile: MediaProfile) -> Result<Vec<Packet>> {
        chunk_native(&self.encode_native()?, profile)
    }

    /// Parse native 16/32 pixel AA frames, including 16 pixel palette reuse.
    /// A truncated trailing frame is an error, never an implicit end of file.
    pub fn decode_native(mut stream: &[u8]) -> Result<Self> {
        let mut frames = Vec::new();
        let mut previous_palette = Vec::new();
        while !stream.is_empty() {
            if stream.len() < 3 || stream[0] != 0xaa {
                return Err(Error::Protocol("invalid native frame marker"));
            }
            let length = usize::from(u16::from_le_bytes([stream[1], stream[2]]));
            if length < 7 || length > stream.len() {
                return Err(Error::Protocol("invalid or truncated native frame length"));
            }
            let body = &stream[3..length];
            stream = &stream[length..];
            if body == [0, 0, 5, 0, 0] || body == [0, 0, 6, 0, 0, 0] {
                continue;
            }
            let duration_ms = u16::from_le_bytes([body[0], body[1]]);
            let (dimension, reuse, count, header_len) = match body[2] {
                0 => (Dimension::Sixteen, false, usize::from(body[3]), 4),
                1 => (Dimension::Sixteen, true, usize::from(body[3]), 4),
                3 if body.len() >= 5 => (
                    Dimension::ThirtyTwo,
                    false,
                    usize::from(u16::from_le_bytes([body[3], body[4]])),
                    5,
                ),
                _ => return Err(Error::Unsupported("unknown native frame palette flag")),
            };
            let side = dimension.pixels();
            let pixel_count = (side * side) as usize;
            let local_count = if count == 0 && !reuse {
                pixel_count
            } else {
                count
            };
            let palette_end = header_len + local_count * 3;
            if palette_end > body.len() {
                return Err(Error::Protocol("truncated native palette"));
            }
            let mut palette = if reuse {
                previous_palette.clone()
            } else {
                Vec::new()
            };
            for rgb in body[header_len..palette_end].chunks_exact(3) {
                palette.push([rgb[0], rgb[1], rgb[2]]);
            }
            if palette.is_empty() || palette.len() > pixel_count {
                return Err(Error::Protocol("invalid native palette size"));
            }
            let bpp = bits_per_pixel(palette.len())?;
            let packed = &body[palette_end..];
            if packed.len() != (pixel_count * bpp).div_ceil(8) {
                return Err(Error::Protocol("invalid native pixel data length"));
            }
            let mut image = RgbaImage::new(side, side);
            for (i, pixel) in image.pixels_mut().enumerate() {
                let mut index = 0usize;
                for bit in 0..bpp {
                    let offset = i * bpp + bit;
                    index |= usize::from((packed[offset / 8] >> (offset % 8)) & 1) << bit;
                }
                let rgb = palette
                    .get(index)
                    .ok_or(Error::Protocol("native pixel index exceeds palette"))?;
                *pixel = Rgba([rgb[0], rgb[1], rgb[2], 255]);
            }
            previous_palette = palette;
            frames.push(Frame::new(image, duration_ms)?);
        }
        Self::new(frames)
    }

    /// Decode and composite GIF rectangles before resizing. Delay values are
    /// retained exactly in milliseconds, including zero. Delays above u16 fail.
    /// Logical canvases above 16 million pixels or over 4096 frames are rejected.
    pub fn from_gif(reader: impl Read, dimension: Dimension) -> Result<Self> {
        Self::from_gif_fitted(reader, dimension, FitMode::Stretch)
    }

    /// Composite GIF disposal before applying the selected fit policy.
    pub fn from_gif_fitted(reader: impl Read, dimension: Dimension, fit: FitMode) -> Result<Self> {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options
            .read_info(reader)
            .map_err(|error| Error::Media(error.to_string()))?;
        let width = u32::from(decoder.width());
        let height = u32::from(decoder.height());
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_000_000 {
            return Err(Error::Invalid(
                "GIF canvas is empty or exceeds 16 million pixels",
            ));
        }
        let background = decoder
            .bg_color()
            .and_then(|index| {
                let palette = decoder.global_palette()?;
                let color = palette.get(index * 3..index * 3 + 3)?;
                Some(Rgba([color[0], color[1], color[2], 255]))
            })
            .unwrap_or(Rgba([0, 0, 0, 0]));
        let mut canvas = RgbaImage::from_pixel(width, height, background);
        let mut frames = Vec::new();
        while let Some(frame) = decoder
            .read_next_frame()
            .map_err(|error| Error::Media(error.to_string()))?
        {
            if frames.len() == 4096 {
                return Err(Error::Invalid("GIF exceeds 4096 frames"));
            }
            if frame.needs_user_input {
                return Err(Error::Unsupported("GIF requires interactive user input"));
            }
            let left = u32::from(frame.left);
            let top = u32::from(frame.top);
            let fw = u32::from(frame.width);
            let fh = u32::from(frame.height);
            if fw == 0
                || fh == 0
                || left + fw > width
                || top + fh > height
                || frame.buffer.len() != (fw * fh * 4) as usize
            {
                return Err(Error::Invalid("GIF frame exceeds its canvas"));
            }
            let previous = (frame.dispose == gif::DisposalMethod::Previous).then(|| canvas.clone());
            for y in 0..fh {
                for x in 0..fw {
                    let offset = ((y * fw + x) * 4) as usize;
                    let rgba = &frame.buffer[offset..offset + 4];
                    if rgba[3] != 0 {
                        canvas.put_pixel(
                            left + x,
                            top + y,
                            Rgba([rgba[0], rgba[1], rgba[2], rgba[3]]),
                        );
                    }
                }
            }
            let duration_ms = u16::try_from(u32::from(frame.delay) * 10)
                .map_err(|_| Error::Invalid("GIF delay exceeds 65535 milliseconds"))?;
            frames.push(Frame::from_image_fitted(
                &DynamicImage::ImageRgba8(canvas.clone()),
                dimension,
                duration_ms,
                fit,
            )?);
            match frame.dispose {
                gif::DisposalMethod::Background => {
                    for y in top..top + fh {
                        for x in left..left + fw {
                            canvas.put_pixel(x, y, background);
                        }
                    }
                }
                gif::DisposalMethod::Previous => {
                    canvas = previous.ok_or(Error::Protocol("missing previous GIF canvas"))?
                }
                _ => {}
            }
        }
        Self::new(frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Start,
    Center,
    End,
}

/// Rasterized text or another monochrome graphic. Each column's bit 0 is its
/// top row. Font selection and shaping belong to the caller's font renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBitmap {
    columns: Vec<u64>,
    height: u8,
}

impl TextBitmap {
    pub fn new(columns: Vec<u64>, height: u8) -> Result<Self> {
        if columns.is_empty() || columns.len() > 4096 || !(1..=64).contains(&height) {
            return Err(Error::Invalid(
                "bitmap needs 1..4096 columns and height 1..64",
            ));
        }
        if height < 64 && columns.iter().any(|column| column >> height != 0) {
            return Err(Error::Invalid(
                "bitmap has pixels below its declared height",
            ));
        }
        Ok(Self { columns, height })
    }

    pub fn columns(&self) -> &[u64] {
        &self.columns
    }
    pub const fn height(&self) -> u8 {
        self.height
    }

    pub fn static_frame(
        &self,
        dimension: Dimension,
        horizontal: Alignment,
        vertical: Alignment,
        foreground: [u8; 3],
        background: [u8; 3],
    ) -> Result<Frame> {
        let x = aligned_origin(
            dimension.pixels() as i64,
            self.columns.len() as i64,
            horizontal,
        );
        let y = aligned_origin(dimension.pixels() as i64, i64::from(self.height), vertical);
        self.render(dimension, x, y, foreground, background, 0)
    }

    /// Move left one column per frame, with a blank first and last frame.
    pub fn scrolling(
        &self,
        dimension: Dimension,
        vertical: Alignment,
        foreground: [u8; 3],
        background: [u8; 3],
        duration_ms: u16,
    ) -> Result<Animation> {
        let side = i64::from(dimension.pixels());
        let y = aligned_origin(side, i64::from(self.height), vertical);
        let mut frames = Vec::new();
        for x in (-(self.columns.len() as i64)..=side).rev() {
            frames.push(self.render(dimension, x, y, foreground, background, duration_ms)?);
        }
        Animation::new(frames)
    }

    fn render(
        &self,
        dimension: Dimension,
        origin_x: i64,
        origin_y: i64,
        foreground: [u8; 3],
        background: [u8; 3],
        duration_ms: u16,
    ) -> Result<Frame> {
        let rgba = |rgb: [u8; 3]| Rgba([rgb[0], rgb[1], rgb[2], 255]);
        let side = dimension.pixels();
        let mut image = RgbaImage::from_pixel(side, side, rgba(background));
        for y in 0..side {
            for x in 0..side {
                let source_x = i64::from(x) - origin_x;
                let source_y = i64::from(y) - origin_y;
                if source_x >= 0
                    && source_x < self.columns.len() as i64
                    && source_y >= 0
                    && source_y < i64::from(self.height)
                    && self.columns[source_x as usize] >> source_y & 1 != 0
                {
                    image.put_pixel(x, y, rgba(foreground));
                }
            }
        }
        Frame::new(image, duration_ms)
    }
}

fn aligned_origin(available: i64, content: i64, alignment: Alignment) -> i64 {
    match alignment {
        Alignment::Start => 0,
        Alignment::Center => (available - content).div_euclid(2),
        Alignment::End => available - content,
    }
}

fn wrap_frame(body: &[u8]) -> Result<Vec<u8>> {
    let len = u16::try_from(body.len() + 3)
        .map_err(|_| Error::Invalid("native frame exceeds u16 length"))?;
    let mut frame = vec![0xaa];
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

fn bits_per_pixel(palette_len: usize) -> Result<usize> {
    if palette_len == 0 || palette_len > 4096 {
        return Err(Error::Invalid("palette size must be 1..4096"));
    }
    Ok((usize::BITS - (palette_len - 1).leading_zeros()).max(1) as usize)
}

/// Minimal LSB-first packing. One-color palettes still use one bit per pixel.
pub fn pack_pixels(pixels: &[u16], palette_len: usize) -> Result<Vec<u8>> {
    let bpp = bits_per_pixel(palette_len)?;
    let bit_count = pixels
        .len()
        .checked_mul(bpp)
        .ok_or(Error::Invalid("pixel data length overflow"))?;
    let mut output = vec![0; bit_count.div_ceil(8)];
    for (i, &index) in pixels.iter().enumerate() {
        if usize::from(index) >= palette_len {
            return Err(Error::Invalid("pixel index exceeds palette"));
        }
        for bit in 0..bpp {
            output[(i * bpp + bit) / 8] |= (((index >> bit) & 1) as u8) << ((i * bpp + bit) % 8);
        }
    }
    Ok(output)
}

/// Split an opaque native stream into fixed 200-byte command 0x49 pieces.
/// This does not reinterpret or validate externally supplied native frame data.
pub fn chunk_native(stream: &[u8], profile: MediaProfile) -> Result<Vec<Packet>> {
    if stream.is_empty() {
        return Err(Error::Invalid("native stream is empty"));
    }
    let chunk_count = stream.len().div_ceil(200);
    match profile {
        MediaProfile::Standard if stream.len() > usize::from(u16::MAX) || chunk_count > 256 => {
            return Err(Error::Invalid(
                "animation exceeds standard size or chunk counter",
            ))
        }
        MediaProfile::PixooMax if u32::try_from(stream.len()).is_err() || chunk_count > 65536 => {
            return Err(Error::Invalid(
                "animation exceeds Pixoo Max size or chunk counter",
            ))
        }
        _ => {}
    }
    let mut packets = Vec::with_capacity(chunk_count);
    for (index, chunk) in stream.chunks(200).enumerate() {
        let mut payload = Vec::new();
        match profile {
            MediaProfile::Standard => {
                payload.extend_from_slice(
                    &u16::try_from(stream.len())
                        .map_err(|_| Error::Invalid("animation size overflow"))?
                        .to_le_bytes(),
                );
                payload.push(
                    u8::try_from(index).map_err(|_| Error::Invalid("animation index overflow"))?,
                );
            }
            MediaProfile::PixooMax => {
                payload.extend_from_slice(
                    &u32::try_from(stream.len())
                        .map_err(|_| Error::Invalid("animation size overflow"))?
                        .to_le_bytes(),
                );
                payload.extend_from_slice(
                    &u16::try_from(index)
                        .map_err(|_| Error::Invalid("animation index overflow"))?
                        .to_le_bytes(),
                );
            }
        }
        payload.extend_from_slice(chunk);
        packets.push(Packet::new(0x49, payload)?);
    }
    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }

    fn goldens() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/go-goldens.json")).unwrap()
    }

    fn solid(dimension: Dimension, rgb: [u8; 3], duration_ms: u16) -> Frame {
        Frame::new(
            RgbaImage::from_pixel(
                dimension.pixels(),
                dimension.pixels(),
                Rgba([rgb[0], rgb[1], rgb[2], 255]),
            ),
            duration_ms,
        )
        .unwrap()
    }

    fn checker(duration_ms: u16) -> Frame {
        Frame::new(
            RgbaImage::from_fn(32, 32, |x, y| {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                Rgba([value, value, value, 255])
            }),
            duration_ms,
        )
        .unwrap()
    }

    fn assert_packet_fixture(packet: &Packet, expected: &str) {
        let bytes = hex(expected);
        assert_eq!(packet.opcode(), bytes[3]);
        assert_eq!(packet.payload(), &bytes[4..bytes.len() - 3]);
        assert_eq!(
            crate::protocol::encode(packet, crate::protocol::ClassicDialect::PixooMax).unwrap(),
            bytes
        );
    }

    #[test]
    fn packing_matches_independent_go_goldens() {
        let fixtures = goldens();
        for (key, pixels, count) in [
            ("pack_1color_16px", vec![0; 16], 1),
            ("pack_2color_alt_16px", [0, 1].repeat(8), 2),
            ("pack_3color_8px", vec![0, 1, 2, 0, 1, 2, 0, 1], 3),
            ("pack_5color_8px", vec![0, 1, 2, 3, 4, 0, 1, 2], 5),
        ] {
            assert_eq!(
                pack_pixels(&pixels, count).unwrap(),
                hex(fixtures[key].as_str().unwrap())
            );
        }
        assert!(pack_pixels(&[3], 3).is_err());
        assert!(pack_pixels(&[], 0).is_err());
    }

    #[test]
    fn frames_and_static_packet_match_go_goldens() {
        let fixtures = goldens();
        let red = solid(Dimension::Sixteen, [255, 0, 0], 0);
        assert_eq!(
            red.encode_body(0).unwrap(),
            hex(fixtures["frame16_red_single"].as_str().unwrap())
        );
        assert_eq!(
            red.encode_native().unwrap(),
            hex(fixtures["makeframe16_red"].as_str().unwrap())
        );
        assert_eq!(
            checker(0).encode_body(0).unwrap(),
            hex(fixtures["frame32_checker_single"].as_str().unwrap())
        );
        assert_packet_fixture(
            &red.static_packet().unwrap(),
            fixtures["setimage16_red_full_message"].as_str().unwrap(),
        );
    }

    #[test]
    fn narrow_content_on_max_and_wide_content_match_go_goldens() {
        let fixtures = goldens();
        let small = Animation::new(vec![
            solid(Dimension::Sixteen, [255, 0, 0], 500),
            solid(Dimension::Sixteen, [0, 0, 0], 500),
        ])
        .unwrap();
        let wide = Animation::new(vec![
            checker(250),
            solid(Dimension::ThirtyTwo, [255, 0, 0], 250),
        ])
        .unwrap();
        for (animation, key) in [
            (&small, "anim_2frames_messages"),
            (&wide, "anim32_2frames_messages"),
        ] {
            let packets = animation.packets(MediaProfile::PixooMax).unwrap();
            let expected = fixtures[key].as_array().unwrap();
            assert_eq!(packets.len(), expected.len());
            for (packet, fixture) in packets.iter().zip(expected) {
                assert_packet_fixture(packet, fixture.as_str().unwrap());
            }
        }
        let encoded = wide.encode_native().unwrap();
        assert_eq!(
            &encoded[..8],
            hex(fixtures["flag_frame_1"].as_str().unwrap())
        );
        assert_eq!(
            &encoded[8..17],
            hex(fixtures["flag_frame_2"].as_str().unwrap())
        );
    }

    #[test]
    fn per_frame_timing_and_palette_capacity_round_trip() {
        for side in [16, 32] {
            let image = RgbaImage::from_fn(side, side, |x, y| {
                Rgba([(x + y * side) as u8, ((x + y * side) >> 8) as u8, 43, 255])
            });
            let one = Frame::new(image.clone(), 123).unwrap();
            let two = Frame::new(image, 456).unwrap();
            let animation = Animation::new(vec![one, two]).unwrap();
            assert_eq!(
                Animation::decode_native(&animation.encode_native().unwrap()).unwrap(),
                animation
            );
        }
        let single = Animation::new(vec![solid(Dimension::Sixteen, [2, 3, 4], 300)]).unwrap();
        assert_eq!(
            Animation::decode_native(&single.encode_native().unwrap())
                .unwrap()
                .frames()[0]
                .duration_ms(),
            0
        );
    }

    #[test]
    fn alpha_threshold_uses_straight_channels() {
        let mut image = RgbaImage::from_pixel(16, 16, Rgba([10, 20, 30, 33]));
        image.put_pixel(0, 0, Rgba([100, 120, 140, 32]));
        let frame = Frame::new(image, 17).unwrap();
        let decoded = Animation::decode_native(&frame.encode_native().unwrap()).unwrap();
        assert_eq!(
            decoded.frames()[0].image().get_pixel(0, 0).0,
            [0, 0, 0, 255]
        );
        assert_eq!(
            decoded.frames()[0].image().get_pixel(1, 0).0,
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn malformed_native_data_fails_without_panicking() {
        let valid = solid(Dimension::Sixteen, [0, 0, 0], 0)
            .encode_native()
            .unwrap();
        for end in 0..valid.len() {
            assert!(
                Animation::decode_native(&valid[..end]).is_err(),
                "truncation {end}"
            );
        }
        let mut bad_index = valid.clone();
        *bad_index.last_mut().unwrap() = 255;
        assert!(Animation::decode_native(&bad_index).is_err());
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(Animation::decode_native(&trailing).is_err());
        let mut long_frame = valid;
        long_frame[1] += 1;
        assert!(Animation::decode_native(&long_frame).is_err());
        assert!(Animation::new(vec![
            solid(Dimension::Sixteen, [0; 3], 0),
            solid(Dimension::ThirtyTwo, [0; 3], 0)
        ])
        .is_err());
        assert!(solid(Dimension::SixtyFour, [0; 3], 0)
            .encode_native()
            .is_err());
    }

    #[test]
    fn native_palette_reuse_appends_and_references_previous_entries() {
        let red = solid(Dimension::Sixteen, [255, 0, 0], 10);
        let mut stream = red.encode_native().unwrap();
        let mut body = vec![20, 0, 1, 1, 0, 255, 0];
        body.extend_from_slice(&[0xaa; 32]);
        stream.extend(wrap_frame(&body).unwrap());
        let animation = Animation::decode_native(&stream).unwrap();
        let frame = &animation.frames()[1];
        assert_eq!(frame.duration_ms(), 20);
        assert_eq!(frame.image().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(frame.image().get_pixel(1, 0).0, [0, 255, 0, 255]);
    }

    #[test]
    fn counters_are_checked_before_any_packets_are_built() {
        let standard = chunk_native(&vec![0; 201], MediaProfile::Standard).unwrap();
        assert_eq!(standard[0].payload().len(), 203);
        assert_eq!(&standard[1].payload()[..3], &[201, 0, 1]);
        assert_eq!(standard[1].payload().len(), 4);
        assert!(chunk_native(&vec![0; 51201], MediaProfile::Standard).is_err());
        assert!(chunk_native(&vec![0; 13_107_201], MediaProfile::PixooMax).is_err());
        assert!(chunk_native(&[], MediaProfile::PixooMax).is_err());
    }

    fn gif_rect(
        index: u8,
        left: u16,
        dispose: gif::DisposalMethod,
        delay: u16,
    ) -> gif::Frame<'static> {
        gif::Frame {
            width: 1,
            height: 1,
            left,
            dispose,
            delay,
            buffer: Cow::Owned(vec![index]),
            ..gif::Frame::default()
        }
    }

    #[test]
    fn gif_background_previous_transparency_and_delays_are_composited() {
        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(
                &mut bytes,
                16,
                16,
                &[0, 0, 255, 255, 0, 0, 0, 255, 0, 255, 255, 255],
            )
            .unwrap();
            encoder
                .write_frame(&gif_rect(1, 0, gif::DisposalMethod::Keep, 1))
                .unwrap();
            encoder
                .write_frame(&gif_rect(2, 0, gif::DisposalMethod::Previous, 2))
                .unwrap();
            encoder
                .write_frame(&gif_rect(3, 1, gif::DisposalMethod::Background, 3))
                .unwrap();
            let mut transparent = gif_rect(2, 0, gif::DisposalMethod::Keep, 0);
            transparent.transparent = Some(2);
            encoder.write_frame(&transparent).unwrap();
        }
        let animation = Animation::from_gif(bytes.as_slice(), Dimension::Sixteen).unwrap();
        assert_eq!(
            animation
                .frames()
                .iter()
                .map(Frame::duration_ms)
                .collect::<Vec<_>>(),
            vec![10, 20, 30, 0]
        );
        assert_eq!(
            animation.frames()[0].image().get_pixel(1, 0).0,
            [0, 0, 255, 255]
        );
        assert_eq!(
            animation.frames()[1].image().get_pixel(0, 0).0,
            [0, 255, 0, 255]
        );
        assert_eq!(
            animation.frames()[2].image().get_pixel(0, 0).0,
            [255, 0, 0, 255]
        );
        assert_eq!(
            animation.frames()[2].image().get_pixel(1, 0).0,
            [255, 255, 255, 255]
        );
        assert_eq!(
            animation.frames()[3].image().get_pixel(0, 0).0,
            [255, 0, 0, 255]
        );
        assert_eq!(
            animation.frames()[3].image().get_pixel(1, 0).0,
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn gif_invalid_data_and_unrepresentable_delays_fail() {
        assert!(Animation::from_gif(&b"not a gif"[..], Dimension::Sixteen).is_err());
        let mut bytes = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut bytes, 16, 16, &[0, 0, 0, 255, 255, 255]).unwrap();
            encoder
                .write_frame(&gif_rect(1, 0, gif::DisposalMethod::Keep, 6554))
                .unwrap();
        }
        assert!(Animation::from_gif(bytes.as_slice(), Dimension::Sixteen).is_err());
    }

    #[test]
    fn text_bitmap_static_alignment_and_scroll_endpoints() {
        let bitmap = TextBitmap::new(vec![1, 2], 2).unwrap();
        let frame = bitmap
            .static_frame(
                Dimension::Sixteen,
                Alignment::Center,
                Alignment::End,
                [255; 3],
                [0; 3],
            )
            .unwrap();
        assert_eq!(frame.image().get_pixel(7, 14).0, [255; 4]);
        assert_eq!(frame.image().get_pixel(8, 15).0, [255; 4]);
        let animation = bitmap
            .scrolling(Dimension::Sixteen, Alignment::Start, [255; 3], [0; 3], 50)
            .unwrap();
        assert_eq!(animation.frames().len(), 19);
        assert!(animation.frames()[0]
            .image()
            .pixels()
            .all(|pixel| pixel[0] == 0));
        assert!(animation
            .frames()
            .last()
            .unwrap()
            .image()
            .pixels()
            .all(|pixel| pixel[0] == 0));
        assert_eq!(animation.frames()[1].image().get_pixel(15, 0).0, [255; 4]);
        assert_eq!(animation.frames()[1].duration_ms(), 50);
        assert!(TextBitmap::new(vec![4], 2).is_err());
        assert!(TextBitmap::new(vec![0], 0).is_err());
    }
}
