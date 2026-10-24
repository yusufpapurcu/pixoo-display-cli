//! Checked light, image, drawing and storage commands from official pages 287–313.
//!
//! Encoded assets are opaque outputs of the named Divoom encoder. In particular,
//! page 292 does not specify the bytes produced by `divoom_image_encode_frame_word`.
//! No text or image encoder is implied by accepting an encoded block here.
use super::{Percent, Rgb};
use crate::{
    protocol::{Packet, Reply},
    Error, Result,
};

/// Already encoded picture/animation bytes. The proprietary codec is not described
/// by pages 289–290. This type checks transport bounds, not the media format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedImageBlock(Vec<u8>);
impl EncodedImageBlock {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::Invalid("encoded image must not be empty"));
        }
        if bytes.len() > u16::MAX as usize {
            return Err(Error::Invalid("encoded image exceeds u16 length"));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Output of `divoom_image_encode_frame_word`, not UTF-8 or a guessed structure dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedWordFrame(Vec<u8>);
impl EncodedWordFrame {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > u16::MAX as usize {
            return Err(Error::Invalid(
                "encoded word frame length must be 1..=65535",
            ));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Native text-input information described on page 292. The 32-byte bitmap
/// represents a 16x16 glyph. This is an input model, never a wire struct dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordFont {
    pub unicode16: u16,
    pub font_info: [u8; 32],
}

/// Structural input for the undocumented frame-word encoder. The page omits
/// DIVOOM_IMAGE_ENCODE_WORD_MAX_NUM and the codec. Counts derive from the vectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordInfo {
    pub x: u8,
    pub y: u8,
    pub size: FontSize,
    pub width: u8,
    pub height: u8,
    pub font_effect: u8,
    pub picture_effect: u8,
    pub speed_ms: u16,
    pub color: Rgb,
    pub unicode: Vec<u16>,
    pub fonts: Vec<WordFont>,
}
impl WordInfo {
    /// Checks only the specified structural constraints, not the omitted native maximum.
    pub fn validate(&self) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.unicode.len() > u16::MAX as usize
            || self.fonts.len() > u8::MAX as usize
        {
            return Err(Error::Invalid(
                "word information exceeds structural field bounds",
            ));
        }
        for (i, font) in self.fonts.iter().enumerate() {
            if self.fonts[..i]
                .iter()
                .any(|f| f.unicode16 == font.unicode16)
            {
                return Err(Error::Invalid(
                    "word font table must contain unique characters",
                ));
            }
        }
        if self
            .unicode
            .iter()
            .any(|ch| !self.fonts.iter().any(|f| f.unicode16 == *ch))
        {
            return Err(Error::Invalid(
                "word font table must cover every text character",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TimeFormat {
    TwelveHour = 0,
    TwentyFourHour = 1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightMode {
    Environment {
        time_format: TimeFormat,
        display_mode: u8,
        checkboxes: [u8; 4],
        color: Rgb,
    },
    Light {
        color: Rgb,
        brightness: Percent,
        effect: u8,
        on: bool,
    },
    Divoom,
    Special {
        selection: u8,
    },
    Music {
        selection: u8,
    },
    UserDefined,
    Score {
        on: bool,
        red: u8,
        blue: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FontSize {
    Points16 = 16,
    Points24 = 24,
    Points32 = 32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextAttribute {
    Speed {
        milliseconds: u16,
        text_box: u8,
    },
    Effect {
        style: u8,
    },
    DisplayBox {
        x: u8,
        y: u8,
        width: u8,
        height: u8,
        text_box: u8,
    },
    Font {
        size: FontSize,
        text_box: u8,
    },
    Color {
        color: Rgb,
        text_box: u8,
    },
    Content {
        frame: EncodedWordFrame,
        text_box: u8,
    },
    ImageEffect {
        style: u8,
        text_box: u8,
    },
}

/// A 200-byte transfer packet. Sequence IDs start at zero. Every nonfinal
/// packet must contain exactly 200 bytes; the final packet contains the remainder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationChunk {
    total: u16,
    sequence: u8,
    data: Vec<u8>,
}
impl AnimationChunk {
    pub fn new(total: u16, sequence: u8, data: Vec<u8>) -> Result<Self> {
        let offset = usize::from(sequence) * 200;
        let total = usize::from(total);
        if total == 0 || total > 256 * 200 || offset >= total {
            return Err(Error::Invalid(
                "animation total or sequence is outside 200-byte transfer range",
            ));
        }
        if data.len() != (total - offset).min(200) {
            return Err(Error::Invalid(
                "animation chunk must contain 200 bytes or exact final remainder",
            ));
        }
        Ok(Self {
            total: total as u16,
            sequence,
            data,
        })
    }
    pub fn total(&self) -> u16 {
        self.total
    }
    pub fn sequence(&self) -> u8 {
        self.sequence
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    fn append(&self, out: &mut Vec<u8>) {
        out.extend(self.total.to_le_bytes());
        out.push(self.sequence);
        out.extend(&self.data);
    }
    pub fn decode_upload(reply: &Reply) -> Result<(Self, Vec<u8>)> {
        let d = reply_data(reply, 0x49, 3)?;
        let total = u16::from_le_bytes([d[0], d[1]]);
        let offset = usize::from(d[2]) * 200;
        if offset >= usize::from(total) {
            return Err(Error::Protocol("animation upload sequence outside total"));
        }
        let len = (usize::from(total) - offset).min(200);
        if d.len() < 3 + len {
            return Err(Error::Protocol("truncated animation upload chunk"));
        }
        let chunk = Self::new(total, d[2], d[3..3 + len].to_vec())
            .map_err(|_| Error::Protocol("invalid animation upload chunk"))?;
        Ok((chunk, d[3 + len..].to_vec()))
    }
}

/// 256-byte file-transfer chunk for 0x8B/0x8C/0x8D. The ID is a packet
/// number, not a byte offset. No more than 65536 packets can be addressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunk {
    total: u32,
    sequence: u16,
    data: Vec<u8>,
}
impl FileChunk {
    pub fn new(total: u32, sequence: u16, data: Vec<u8>) -> Result<Self> {
        check_file_size(total)?;
        let offset = u32::from(sequence) * 256;
        if offset >= total || data.len() != ((total - offset) as usize).min(256) {
            return Err(Error::Invalid(
                "file chunk must contain 256 bytes or exact final remainder",
            ));
        }
        Ok(Self {
            total,
            sequence,
            data,
        })
    }
    pub fn total(&self) -> u32 {
        self.total
    }
    pub fn sequence(&self) -> u16 {
        self.sequence
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    fn append(&self, out: &mut Vec<u8>) {
        out.extend(self.total.to_le_bytes());
        out.extend(self.sequence.to_le_bytes());
        out.extend(&self.data);
    }
}
fn check_file_size(size: u32) -> Result<()> {
    if size == 0 || size > 65536 * 256 {
        return Err(Error::Invalid(
            "file size must fit 65536 chunks of 256 bytes",
        ));
    }
    Ok(())
}

/// Four-byte file ID from page 298. Global page 146 specifies little-endian
/// order for multibyte fields. The original bytes remain available for exact reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtworkId(pub [u8; 4]);
impl ArtworkId {
    pub fn from_u32(value: u32) -> Self {
        Self(value.to_le_bytes())
    }
    pub fn value(self) -> u32 {
        u32::from_le_bytes(self.0)
    }
}

/// Page 297 does not specify the width or byte order of File Id. Callers must
/// supply device-confirmed wire bytes. `from_enumerated` explicitly elects to
/// reuse page 298's four-byte IDs; that cross-command equivalence is unverified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedArtworkId(Vec<u8>);
impl EncodedArtworkId {
    pub fn from_wire(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > u16::MAX as usize - 16 {
            return Err(Error::Invalid("artwork id must be nonempty and fit packet"));
        }
        Ok(Self(bytes))
    }
    pub fn from_enumerated(id: ArtworkId) -> Self {
        Self(id.0.to_vec())
    }
    /// Explicit u32LE choice. Four-byte width is inferred from page 298;
    /// page 297 itself omits ID width. Byte order follows global page 146.
    pub fn from_u32_le(id: u32) -> Self {
        Self(id.to_le_bytes().to_vec())
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GifTransfer {
    Start { size: u32 },
    Data(FileChunk),
    End,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedTransfer {
    Start { size: u32, index: u8 },
    Data(FileChunk),
    End,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Big64Transfer {
    Start {
        size: u32,
        index: u8,
        file_id: EncodedArtworkId,
    },
    Data(FileChunk),
    End,
    Delete {
        file_id: EncodedArtworkId,
        index: u8,
    },
    Play {
        file_id: EncodedArtworkId,
        index: u8,
    },
    DeleteIndex {
        index: u8,
    },
}

/// Start/end metadata for 0xB1. LED text is UTF-16 code units, not UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedArtworkKind {
    NormalImage,
    LedEditor {
        speed: u8,
        text: Vec<u16>,
    },
    SandPainting,
    ScrollAnimation {
        mode: u8,
        speed_ms: u16,
        length: u16,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserGif {
    Start(SavedArtworkKind),
    Frame(EncodedImageBlock),
    End(SavedArtworkKind),
}

/// One saved sand-painting operation. A zero count fills the entire image.
/// Offset width is absent from page 294 and is supplied as encoded wire bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandStroke {
    count: u16,
    pub color: Rgb,
    offsets: Vec<u8>,
}
impl SandStroke {
    pub fn from_wire(count: u16, color: Rgb, offsets: Vec<u8>) -> Result<Self> {
        if (count == 0) != offsets.is_empty() {
            return Err(Error::Invalid("sand stroke count and offsets disagree"));
        }
        Ok(Self {
            count,
            color,
            offsets,
        })
    }
    pub fn fill(color: Rgb) -> Self {
        Self {
            count: 0,
            color,
            offsets: Vec::new(),
        }
    }
    pub fn point_count(&self) -> u16 {
        self.count
    }
    pub fn offset_bytes(&self) -> &[u8] {
        &self.offsets
    }
}
/// Saved sand painting consists of speed, an encoded image and operations.
/// Two-byte fields use the little-endian rule from global page 146.
/// No native image codec is supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandPaintingData {
    pub speed_ms: u16,
    pub image: EncodedImageBlock,
    pub strokes: Vec<SandStroke>,
}
impl SandPaintingData {
    pub fn encode_u16_le(&self) -> Result<EncodedImageBlock> {
        let mut bytes = self.speed_ms.to_le_bytes().to_vec();
        append_image(&self.image, &mut bytes);
        for stroke in &self.strokes {
            bytes.extend(stroke.count.to_le_bytes());
            append_rgb(stroke.color, &mut bytes);
            bytes.extend(&stroke.offsets);
        }
        EncodedImageBlock::new(bytes)
    }
}

/// Page 294 describes sand-painting groups but omits point-offset width.
/// Supply already encoded offset bytes with the independent point count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPointOffsets {
    count: u8,
    bytes: Vec<u8>,
}
impl EncodedPointOffsets {
    pub fn from_wire(count: u8, bytes: Vec<u8>) -> Result<Self> {
        if (count == 0) != bytes.is_empty() || bytes.len() > u16::MAX as usize - 16 {
            return Err(Error::Invalid(
                "point count and nonempty encoded offset block must agree",
            ));
        }
        Ok(Self { count, bytes })
    }
    pub fn from_u8(offsets: Vec<u8>) -> Result<Self> {
        let count = u8::try_from(offsets.len())
            .map_err(|_| Error::Invalid("at most 255 drawing points"))?;
        Self::from_wire(count, offsets)
    }
    /// Caller explicitly selects u16LE offsets; the command pages omit their width.
    pub fn from_u16_le(offsets: &[u16]) -> Result<Self> {
        let count = u8::try_from(offsets.len())
            .map_err(|_| Error::Invalid("at most 255 drawing points"))?;
        Self::from_wire(
            count,
            offsets.iter().flat_map(|x| x.to_le_bytes()).collect(),
        )
    }
    pub fn count(&self) -> u8 {
        self.count
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    fn append(&self, out: &mut Vec<u8>) {
        out.push(self.count);
        out.extend(&self.bytes);
    }
}

/// Explicit caller choice for fields whose width is absent from pages 305/306.
/// These choices describe wire bytes, not confirmed device capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnspecifiedLengthWidth {
    U8,
    U16LittleEndian,
    U32LittleEndian,
}
impl UnspecifiedLengthWidth {
    fn append(self, value: usize, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::U8 => out.push(
                u8::try_from(value)
                    .map_err(|_| Error::Invalid("length exceeds selected u8 field"))?,
            ),
            Self::U16LittleEndian => out.extend(
                u16::try_from(value)
                    .map_err(|_| Error::Invalid("length exceeds selected u16 field"))?
                    .to_le_bytes(),
            ),
            Self::U32LittleEndian => out.extend(
                u32::try_from(value)
                    .map_err(|_| Error::Invalid("length exceeds selected u32 field"))?
                    .to_le_bytes(),
            ),
        }
        Ok(())
    }
}

/// Page 303 calls count "Byte 4" but labels it `data[4]`, leaving `data[3]`
/// unspecified. Callers must explicitly choose a layout confirmed on their device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinglePadLayout {
    ConsecutiveByteOrdinals,
    IndexedWithReservedByte(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MovieControl {
    Exit = 0,
    Start = 1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayCommand {
    SetLightMode(LightMode),
    GetLightMode,
    /// Page 289 sends the caller's complete encoder payload unchanged. A native
    /// AA frame may require a device wrapper; this variant does not invent one.
    Picture(EncodedImageBlock),
    PhoneAnimation(AnimationChunk),
    GifSpeed {
        milliseconds: u16,
    },
    Text(TextAttribute),
    GifTransfer(GifTransfer),
    UserGif(UserGif),
    UserGifCount,
    DeleteUserGif {
        item: u8,
    },
    IndexedTransfer(IndexedTransfer),
    Big64Transfer(Big64Transfer),
    GetArtworkInfo {
        index: u8,
    },
    RhythmAnimation {
        position: u8,
        chunk: AnimationChunk,
    },
    EqAnimation {
        position: u8,
        chunk: AnimationChunk,
    },
    DrawMulti {
        screen: u8,
        color: Rgb,
        offsets: EncodedPointOffsets,
    },
    DrawBig {
        canvas_width_screens: u8,
        screen: u8,
        color: Rgb,
        offsets: EncodedPointOffsets,
    },
    DrawSingle {
        color: Rgb,
        offsets: EncodedPointOffsets,
        layout: SinglePadLayout,
    },
    ExitDrawing,
    MultiPicture {
        screen: u8,
        image: EncodedImageBlock,
        length_width: UnspecifiedLengthWidth,
    },
    MultiAnimation {
        screen: u8,
        chunk: AnimationChunk,
        length_width: UnspecifiedLengthWidth,
    },
    PlayMultiAnimation,
    MovieFrame {
        frame_id: u16,
        image: EncodedImageBlock,
    },
    MultiMovieFrame {
        screen: u8,
        frame_id: u16,
        image: EncodedImageBlock,
    },
    Movie(MovieControl),
    EnterOrClearMultiDrawing {
        color: Rgb,
    },
    SandInitialize {
        screen: u8,
        image: EncodedImageBlock,
    },
    SandReset,
    ScrollSettings {
        mode: u8,
        speed_ms: u16,
    },
    ScrollImage(AnimationChunk),
}

impl DisplayCommand {
    /// A required application-level response, distinct from a BLE transport ACK.
    /// Drawing ACKs and file retransmit events are asynchronous/conditional and
    /// must be handled by their workflows rather than blocking every write.
    /// Start 0x8C is acknowledged as 0xB1, as explicitly stated on page 296.
    pub fn response_opcode(&self) -> Option<u8> {
        match self {
            Self::GetLightMode => Some(0x46),
            Self::UserGifCount | Self::DeleteUserGif { .. } => Some(0xb6),
            Self::GetArtworkInfo { .. } => Some(0x8e),
            Self::GifTransfer(GifTransfer::Start { .. }) => Some(0x8b),
            Self::IndexedTransfer(IndexedTransfer::Start { .. }) => Some(0xb1),
            Self::Big64Transfer(Big64Transfer::Start { .. }) => Some(0x8d),
            Self::UserGif(UserGif::Start(_)) => Some(0xb1),
            _ => None,
        }
    }

    pub fn packet(&self) -> Result<Packet> {
        let mut p = Vec::new();
        let opcode = match self {
            Self::SetLightMode(mode) => {
                mode.append(&mut p);
                0x45
            }
            Self::GetLightMode => 0x46,
            Self::Picture(image) => {
                p.extend(image.bytes());
                0x44
            }
            Self::PhoneAnimation(chunk) => {
                chunk.append(&mut p);
                0x49
            }
            Self::GifSpeed { milliseconds } => {
                p.extend(milliseconds.to_le_bytes());
                0x16
            }
            Self::Text(attr) => {
                attr.append(&mut p)?;
                0x87
            }
            Self::GifTransfer(ctrl) => {
                match ctrl {
                    GifTransfer::Start { size } => {
                        check_file_size(*size)?;
                        p.push(0);
                        p.extend(size.to_le_bytes());
                    }
                    GifTransfer::Data(chunk) => {
                        p.push(1);
                        chunk.append(&mut p);
                    }
                    GifTransfer::End => p.push(2),
                }
                0x8b
            }
            Self::UserGif(ctrl) => {
                match ctrl {
                    UserGif::Start(kind) => {
                        p.push(0);
                        kind.append(&mut p)?;
                    }
                    UserGif::Frame(image) => {
                        p.push(1);
                        append_image(image, &mut p);
                    }
                    UserGif::End(kind) => {
                        p.push(2);
                        kind.append(&mut p)?;
                    }
                }
                0xb1
            }
            Self::UserGifCount => {
                p.push(0xff);
                0xb6
            }
            Self::DeleteUserGif { item } => {
                if *item == 0 || *item == 255 {
                    return Err(Error::Invalid(
                        "user item deletion uses 1..=254; 255 queries count",
                    ));
                }
                p.push(*item);
                0xb6
            }
            Self::IndexedTransfer(ctrl) => {
                match ctrl {
                    IndexedTransfer::Start { size, index } => {
                        check_file_size(*size)?;
                        p.push(0);
                        p.extend(size.to_le_bytes());
                        p.push(*index);
                    }
                    IndexedTransfer::Data(chunk) => {
                        p.push(1);
                        chunk.append(&mut p);
                    }
                    IndexedTransfer::End => p.push(2),
                }
                0x8c
            }
            Self::Big64Transfer(ctrl) => {
                match ctrl {
                    Big64Transfer::Start {
                        size,
                        index,
                        file_id,
                    } => {
                        check_file_size(*size)?;
                        p.push(0);
                        p.extend(size.to_le_bytes());
                        p.push(*index);
                        p.extend(file_id.bytes());
                    }
                    Big64Transfer::Data(chunk) => {
                        p.push(1);
                        chunk.append(&mut p);
                    }
                    Big64Transfer::End => p.push(2),
                    Big64Transfer::Delete { file_id, index } => {
                        p.push(3);
                        p.extend(file_id.bytes());
                        p.push(*index);
                    }
                    Big64Transfer::Play { file_id, index } => {
                        p.push(4);
                        p.extend(file_id.bytes());
                        p.push(*index);
                    }
                    Big64Transfer::DeleteIndex { index } => {
                        p.extend([5, *index]);
                    }
                }
                0x8d
            }
            Self::GetArtworkInfo { index } => {
                p.push(*index);
                0x8e
            }
            Self::RhythmAnimation { position, chunk } | Self::EqAnimation { position, chunk } => {
                if *position == 0 {
                    return Err(Error::Invalid("rhythm position starts at one"));
                }
                p.push(*position);
                chunk.append(&mut p);
                if matches!(self, Self::RhythmAnimation { .. }) {
                    0xb7
                } else {
                    0x1b
                }
            }
            Self::DrawMulti {
                screen,
                color,
                offsets,
            } => {
                p.push(*screen);
                append_rgb(*color, &mut p);
                offsets.append(&mut p);
                0x3a
            }
            Self::DrawBig {
                canvas_width_screens,
                screen,
                color,
                offsets,
            } => {
                if *canvas_width_screens == 0 {
                    return Err(Error::Invalid(
                        "canvas width must be at least one 16-pixel screen",
                    ));
                }
                p.extend([*canvas_width_screens, *screen]);
                append_rgb(*color, &mut p);
                offsets.append(&mut p);
                0x3b
            }
            Self::DrawSingle {
                color,
                offsets,
                layout,
            } => {
                append_rgb(*color, &mut p);
                if let SinglePadLayout::IndexedWithReservedByte(b) = layout {
                    p.push(*b);
                }
                offsets.append(&mut p);
                0x58
            }
            Self::ExitDrawing => 0x5a,
            Self::MultiPicture {
                screen,
                image,
                length_width,
            } => {
                p.push(*screen);
                length_width.append(image.bytes().len(), &mut p)?;
                p.extend(image.bytes());
                0x5b
            }
            Self::MultiAnimation {
                screen,
                chunk,
                length_width,
            } => {
                p.push(*screen);
                length_width.append(usize::from(chunk.total), &mut p)?;
                p.push(chunk.sequence);
                p.extend(&chunk.data);
                0x5c
            }
            Self::PlayMultiAnimation => 0x6b,
            Self::MovieFrame { frame_id, image } => {
                p.extend(frame_id.to_le_bytes());
                append_image(image, &mut p);
                0x6c
            }
            Self::MultiMovieFrame {
                screen,
                frame_id,
                image,
            } => {
                p.push(*screen);
                p.extend(frame_id.to_le_bytes());
                append_image(image, &mut p);
                0x6d
            }
            Self::Movie(ctrl) => {
                p.push(*ctrl as u8);
                0x6e
            }
            Self::EnterOrClearMultiDrawing { color } => {
                append_rgb(*color, &mut p);
                0x6f
            }
            Self::SandInitialize { screen, image } => {
                p.extend([0, *screen]);
                append_image(image, &mut p);
                0x34
            }
            Self::SandReset => {
                p.push(1);
                0x34
            }
            Self::ScrollSettings { mode, speed_ms } => {
                p.extend([0, *mode]);
                p.extend(speed_ms.to_le_bytes());
                0x35
            }
            Self::ScrollImage(chunk) => {
                p.push(1);
                chunk.append(&mut p);
                0x35
            }
        };
        Packet::new(opcode, p)
    }
}

impl super::Command for DisplayCommand {
    fn response_is_stream(&self) -> bool {
        matches!(self, Self::GetArtworkInfo { .. })
    }
    fn packet(&self) -> Result<Packet> {
        DisplayCommand::packet(self)
    }
    fn response_opcode(&self) -> Option<u8> {
        DisplayCommand::response_opcode(self)
    }
}

fn append_rgb(color: Rgb, out: &mut Vec<u8>) {
    out.extend([color.r, color.g, color.b]);
}
fn append_image(image: &EncodedImageBlock, out: &mut Vec<u8>) {
    out.extend((image.bytes().len() as u16).to_le_bytes());
    out.extend(image.bytes());
}
impl LightMode {
    fn append(&self, out: &mut Vec<u8>) {
        match self {
            Self::Environment {
                time_format,
                display_mode,
                checkboxes,
                color,
            } => {
                out.extend([0, *time_format as u8, *display_mode]);
                out.extend(checkboxes);
                append_rgb(*color, out);
            }
            Self::Light {
                color,
                brightness,
                effect,
                on,
            } => {
                out.push(1);
                append_rgb(*color, out);
                out.extend([brightness.get(), *effect, u8::from(*on)]);
            }
            Self::Divoom => out.push(2),
            Self::Special { selection } => out.extend([3, *selection]),
            Self::Music { selection } => out.extend([4, *selection]),
            Self::UserDefined => out.push(5),
            Self::Score { on, red, blue } => out.extend([6, u8::from(*on), *red, *blue]),
        }
    }
}
impl TextAttribute {
    fn append(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Speed {
                milliseconds,
                text_box,
            } => {
                out.push(1);
                out.extend(milliseconds.to_le_bytes());
                out.push(*text_box);
            }
            Self::Effect { style } => out.extend([2, *style]),
            Self::DisplayBox {
                x,
                y,
                width,
                height,
                text_box,
            } => {
                if *width == 0
                    || *height == 0
                    || u16::from(*x) + u16::from(*width) > 256
                    || u16::from(*y) + u16::from(*height) > 256
                {
                    return Err(Error::Invalid(
                        "text box must be nonempty and fit byte coordinates",
                    ));
                }
                out.extend([3, *x, *y, *width, *height, *text_box]);
            }
            Self::Font { size, text_box } => out.extend([4, *size as u8, *text_box]),
            Self::Color { color, text_box } => {
                out.push(5);
                append_rgb(*color, out);
                out.push(*text_box);
            }
            Self::Content { frame, text_box } => {
                out.push(6);
                out.extend((frame.bytes().len() as u16).to_le_bytes());
                out.extend(frame.bytes());
                out.push(*text_box);
            }
            Self::ImageEffect { style, text_box } => out.extend([7, *style, *text_box]),
        }
        Ok(())
    }
}
impl SavedArtworkKind {
    fn append(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::NormalImage => out.push(0),
            Self::LedEditor { speed, text } => {
                let len = u8::try_from(text.len())
                    .map_err(|_| Error::Invalid("LED text exceeds 255 UTF-16 code units"))?;
                out.extend([1, *speed, len]);
                for ch in text {
                    out.extend(ch.to_le_bytes());
                }
            }
            Self::SandPainting => out.push(2),
            Self::ScrollAnimation {
                mode,
                speed_ms,
                length,
            } => {
                out.extend([3, *mode]);
                out.extend(speed_ms.to_le_bytes());
                out.extend(length.to_le_bytes());
            }
        }
        Ok(())
    }
}

fn reply_data(reply: &Reply, opcode: u8, minimum: usize) -> Result<&[u8]> {
    if reply.opcode != opcode {
        return Err(Error::Protocol("unexpected display reply opcode"));
    }
    reply.ensure_success()?;
    if reply.data.len() < minimum {
        return Err(Error::Protocol("truncated display reply"));
    }
    Ok(&reply.data)
}

/// Page 288's current modes differ from page 287's set-mode names at value 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentLightMode {
    Clock,
    NightLight,
    Hot,
    Vj,
    MusicEqOrFlag,
    Custom,
    Lyric,
    Unknown(u8),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightSettings {
    pub mode: CurrentLightMode,
    /// Raw field preserves future temperature encodings. 0=Celsius, 1=Fahrenheit.
    pub temperature_mode: u8,
    pub vj_selection: u8,
    pub color: Rgb,
    pub brightness: u8,
    pub light_selection: u8,
    pub light_switch: u8,
    pub music_selection: u8,
    pub system_brightness: u8,
    pub time_format: u8,
    pub time_color: Rgb,
    pub time_display_mode: u8,
    pub time_checkboxes: [u8; 4],
    pub extensions: Vec<u8>,
}
impl LightSettings {
    pub fn decode(reply: &Reply) -> Result<Self> {
        let d = reply_data(reply, 0x46, 20)?;
        let mode = match d[0] {
            0 => CurrentLightMode::Clock,
            1 => CurrentLightMode::NightLight,
            2 => CurrentLightMode::Hot,
            3 => CurrentLightMode::Vj,
            4 => CurrentLightMode::MusicEqOrFlag,
            5 => CurrentLightMode::Custom,
            6 => CurrentLightMode::Lyric,
            x => CurrentLightMode::Unknown(x),
        };
        Ok(Self {
            mode,
            temperature_mode: d[1],
            vj_selection: d[2],
            color: Rgb {
                r: d[3],
                g: d[4],
                b: d[5],
            },
            brightness: d[6],
            light_selection: d[7],
            light_switch: d[8],
            music_selection: d[9],
            system_brightness: d[10],
            time_format: d[11],
            time_color: Rgb {
                r: d[12],
                g: d[13],
                b: d[14],
            },
            time_display_mode: d[15],
            time_checkboxes: [d[16], d[17], d[18], d[19]],
            extensions: d[20..].to_vec(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferReply {
    Ready { allowed: bool, extensions: Vec<u8> },
    Resend { sequence: u16, extensions: Vec<u8> },
    Unknown { control: u8, data: Vec<u8> },
}
impl TransferReply {
    /// 0x8B documents ready/refused and resend. 0x8C documents resend and
    /// acknowledges start through 0xB1. 0x8D documents resend but not start data.
    pub fn decode(reply: &Reply) -> Result<Self> {
        if ![0x8b, 0x8c, 0x8d].contains(&reply.opcode) {
            return Err(Error::Protocol("not a file-transfer reply"));
        }
        let d = reply_data(reply, reply.opcode, 1)?;
        match d[0] {
            0 if reply.opcode == 0x8b => {
                if d.len() < 2 || d[1] > 1 {
                    return Err(Error::Protocol("invalid transfer ready flag"));
                }
                Ok(Self::Ready {
                    allowed: d[1] == 1,
                    extensions: d[2..].to_vec(),
                })
            }
            1 => {
                if d.len() < 3 {
                    return Err(Error::Protocol("truncated resend sequence"));
                }
                Ok(Self::Resend {
                    sequence: u16::from_le_bytes([d[1], d[2]]),
                    extensions: d[3..].to_vec(),
                })
            }
            control => Ok(Self::Unknown {
                control,
                data: d[1..].to_vec(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtworkInfoReply {
    Files {
        index: u8,
        total: u16,
        offset: u16,
        ids: Vec<ArtworkId>,
        extensions: Vec<u8>,
    },
    End {
        index: u8,
        extensions: Vec<u8>,
    },
    Unknown {
        control: u8,
        data: Vec<u8>,
    },
}
impl ArtworkInfoReply {
    pub fn decode(reply: &Reply) -> Result<Self> {
        let d = reply_data(reply, 0x8e, 1)?;
        match d[0] {
            1 => {
                if d.len() < 8 {
                    return Err(Error::Protocol("truncated artwork list header"));
                }
                let total = u16::from_le_bytes([d[2], d[3]]);
                let offset = u16::from_le_bytes([d[4], d[5]]);
                let count = u16::from_le_bytes([d[6], d[7]]);
                if u32::from(offset) + u32::from(count) > u32::from(total) {
                    return Err(Error::Protocol("artwork list exceeds total"));
                }
                let end = 8 + usize::from(count) * 4;
                if d.len() < end {
                    return Err(Error::Protocol("truncated artwork IDs"));
                }
                let ids = d[8..end]
                    .chunks_exact(4)
                    .map(|id| ArtworkId([id[0], id[1], id[2], id[3]]))
                    .collect();
                Ok(Self::Files {
                    index: d[1],
                    total,
                    offset,
                    ids,
                    extensions: d[end..].to_vec(),
                })
            }
            2 => {
                if d.len() < 2 {
                    return Err(Error::Protocol("truncated artwork list end"));
                }
                Ok(Self::End {
                    index: d[1],
                    extensions: d[2..].to_vec(),
                })
            }
            control => Ok(Self::Unknown {
                control,
                data: d[1..].to_vec(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGifItemReply {
    pub item: u8,
    pub extensions: Vec<u8>,
}
impl UserGifItemReply {
    /// For query this is the current count. For deletion it identifies the item.
    /// Hardware notes show B6 counts do not enumerate successful 8C saves on Max.
    pub fn decode(reply: &Reply) -> Result<Self> {
        let d = reply_data(reply, 0xb6, 1)?;
        Ok(Self {
            item: d[0],
            extensions: d[1..].to_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingState {
    RequestWholeImage,
    Display,
    Unknown(u8),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingReply {
    pub state: DrawingState,
    pub extensions: Vec<u8>,
}
impl DrawingReply {
    pub fn decode(reply: &Reply) -> Result<Self> {
        if ![0x3a, 0x3b].contains(&reply.opcode) {
            return Err(Error::Protocol("not a drawing-control reply"));
        }
        let d = reply_data(reply, reply.opcode, 1)?;
        let state = match d[0] {
            0 => DrawingState::RequestWholeImage,
            1 => DrawingState::Display,
            x => DrawingState::Unknown(x),
        };
        Ok(Self {
            state,
            extensions: d[1..].to_vec(),
        })
    }
}

/// 0xB1 acknowledgement data is unspecified and preserved, including Max's
/// observed extra bytes. 0x58 ACK is conditional on entering drawing mode.
pub fn decode_user_gif_ack(reply: &Reply) -> Result<Vec<u8>> {
    Ok(reply_data(reply, 0xb1, 0)?.to_vec())
}
pub fn decode_single_drawing_ack(reply: &Reply) -> Result<Vec<u8>> {
    Ok(reply_data(reply, 0x58, 0)?.to_vec())
}
