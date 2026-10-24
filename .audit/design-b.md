# Rust library design B

This candidate gives each typed request ownership of its command payload and reply interpretation. `Session` owns execution, framing, transport acknowledgments, and transaction boundaries. Callers use the same checked request with pure codecs or a live connection. This is a design sketch only.

## Caller contract

```rust
use divoom::{commands, media, model::*, Session};

let brightness = Percent::try_from(70)?;
let command = commands::display::SetBrightness::new(brightness);
let frame = codec::classic::encode(&command.command()?, ClassicDialect::PixooMax)?;

let mut session = Session::new(transport, DeviceProfile::pixoo_max(), limits);
session.execute(command, deadline).await?;
let volume: Percent = session.execute(commands::audio::GetVolume, deadline).await?;

let artwork = media::Animation::new(frames)?;
let encoded = media::native::encode(&artwork, NativeFormat::PixooMax32)?;
let saved = session.save_indexed(IndexedSlot::try_from(0)?, &encoded, deadline).await?;
assert!(matches!(saved.completion, SaveCompletion::ModeChangedToCustom(_)));
```

The compiler distinguishes indexed upload slots starting at zero, saved-item deletion identifiers starting at one, and saved custom tab selections starting at one. Neither `u8` nor one generic slot type crosses those boundaries.

## Module ownership

| Module | Knowledge owned |
|---|---|
| `model` | Checked scalar values, device profiles, model capabilities, date/time, schedules, colors, coordinates, units, and distinct identifiers. |
| `command` | Logical `Command`, borrowed `Reply`, negative acknowledgment status, request trait, response matching dispositions, and delivery evidence. |
| `commands::{audio,display,clock,weather,alarm,sleep,planner,tools,games,storage,notifications,radio,power,device,resources}` | Official payload layouts and typed reply decoding for their own protocol family. Extended selector layouts live with their domain family. |
| `codec::{classic,ble}` | Pure framing, checksum, escaping, incremental decoding, and BLE envelope encoding. No device connection or clock mutation. |
| `media::{native,image,gif,text,font}` | Validated image buffers, frame timing, palettes, bit packing, AA containers, optional file adapters, and text rasterization. |
| `session` | Exclusive connection ownership, deadlines, reply routing, transport acknowledgments, readiness probes, and delivery uncertainty. |
| `session::transfers` | Stateful storage and media workflows, replay requests, legal phases, bounded retries, and completion evidence. |
| `transport::{rfcomm,ble,serial}` | Platform handles, connection leases, write limits, read boundaries, and resource release. No command payload layouts. |
| `catalogue` | Official page provenance and supported request inventory. The inventory is generated from one declarative command registry. |

Use one library crate under `rust`, with no executable targets. `codec`, typed requests, and native media work without a transport feature. Optional transport dependencies compile only on their target platforms. Optional GIF and font dependencies do not enter the smallest codec build.

## Checked domain shapes

```rust
pub struct Percent(u8);                 // 0..=100, private field
pub struct IndexedSlot(u8);             // zero-based upload destination
pub struct SavedItemId(NonZeroU8);       // one-based deletion target
pub struct CustomTab(NonZeroU8);         // profile validates available count
pub struct ChunkId(u16);
pub struct FrameDelay(u16);             // exact milliseconds, reject overflow
pub struct Rgb { pub red: u8, pub green: u8, pub blue: u8 }
pub struct CanvasSize { width: NonZeroU16, height: NonZeroU16 }
pub struct Weekdays(u8);                // bits 0..6 only
pub struct ClockTime { hour: Hour, minute: Minute }
pub struct AlarmIndex(u8);              // profile-specific count checked at execute
pub enum AlarmTrigger { Music(Frequency), Image(AlarmArtwork) }
pub enum QueryOrSet<T> { Query, Set(T) } // never encode a magic 0xff from an unchecked value

pub struct DeviceProfile {
    classic: ClassicDialect,
    native: NativeFormat,
    transfer: TransferLimits,
    capabilities: Capabilities,
}

impl TryFrom<u8> for Percent { /* checked constructor */ }
impl TryFrom<u8> for Weekdays { /* reject reserved high bit */ }
impl CanvasSize { pub fn new(width: u16, height: u16) -> Result<Self, ValueError>; }
```

Only document-backed ranges become closed enums. Unknown response enum values remain representable with their raw discriminant. Alarm mode and trigger mode need separate types because their numeric spaces differ. Raster dimensions and encoded byte lengths use checked arithmetic before allocation.

## Request trait

```rust
pub struct Command { opcode: u8, data: Vec<u8> }
impl Command {
    pub fn new(opcode: u8, data: Vec<u8>) -> Result<Self, EncodeError>;
    pub fn opcode(&self) -> u8;
    pub fn data(&self) -> &[u8];
}

pub struct Reply<'a> {
    pub opcode: u8,
    pub status: AckStatus,
    pub data: &'a [u8],
}

pub enum ReplyMatch<T> {
    Unrelated,
    Matched(T),
    Rejected(DeviceStatus),
    Malformed(DecodeError),
}

pub trait Request {
    type Response;
    fn command(&self) -> Result<Command, EncodeError>;
    fn reply_policy(&self) -> ReplyPolicy;
    fn classify_reply(&self, reply: Reply<'_>) -> ReplyMatch<Self::Response>;
    fn retry_safety(&self) -> RetrySafety;
}

pub struct SetBrightness { brightness: Percent }
impl Request for SetBrightness {
    type Response = DeliveryReceipt;
    // Written-only on classic, transport acknowledgment on BLE unless command reply requested.
}
pub struct GetVolume;
impl Request for GetVolume { type Response = Percent; }

pub struct RawRequest { command: Command, expected: ExpectedReply }
impl Request for RawRequest { type Response = OwnedReply; }
```

`Command` is a deliberate public codec input, not a platform type. There is no central command enum requiring a second switch over every typed request. A new request normally changes one domain file and its registry entry. Raw access cannot claim typed support for a page whose codec has not been written.

`ReplyPolicy` distinguishes no application reply, required reply, and optional reply. The transport always applies its own acknowledgment policy. A required reply names possible opcodes and any selector prefix, then the request parses the remaining payload. A malformed matching response stops that request with a parse error. An unrelated response enters the bounded event queue.

For setters with no documented application reply, successful execution reports the strongest evidence actually observed. `DeliveryReceipt` distinguishes `Written`, `TransportAcknowledged`, and `CommandAcknowledged`. An acknowledgment must not become an implicit state readback.

## Pure codec signatures

```rust
pub enum ClassicDialect { OfficialEscaped, PixooMax }
pub fn encode(command: &Command, dialect: ClassicDialect) -> Result<Vec<u8>, EncodeError>;

pub struct ClassicDecoder { /* dialect, bounded buffer, partial escape state */ }
impl ClassicDecoder {
    pub fn new(dialect: ClassicDialect, limits: DecodeLimits) -> Self;
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<OwnedFrame>, DecodeError>;
    pub fn finish(&mut self) -> Result<(), DecodeError>;
}

pub enum BleAcknowledgment { None, Required(PacketNumber) }
pub struct PacketNumber(u32);
pub fn encode_envelope(command: &Command, ack: BleAcknowledgment) -> Result<Vec<u8>, EncodeError>;
pub fn decode_ack(reply: Reply<'_>) -> Result<Option<BleAck>, DecodeError>;
pub fn decode_reply(frame: &OwnedFrame) -> Result<Reply<'_>, DecodeError>;
```

Classic length counts its own two bytes plus opcode plus data. Official escaping is applied after length and checksum calculation. The official checksum is the wrapping u16 sum of unescaped length, opcode, and data bytes. Page146 requires escaping but does not give the substitution table. The exact escape mapping needs a sourced fixture before claiming interoperability. Encode and decode must agree about escaping checksum bytes and retain a partial escape across feeds.

Pixoo Max is a separate explicit dialect. `vendor/go-divoom/frame.go` sends unescaped bytes and a u32LE checksum when the byte sum is greater than or equal to `0xffff`. Exactly `0xffff` must select four bytes. Max decoding knows the declared unescaped payload length, computes its sum, then derives whether two or four checksum bytes follow. Embedded `0x01` and `0x02` are payload bytes, so scanning for a footer is incorrect. `finish()` rejects a truncated frame instead of treating unexpected EOF as a valid end.

BLE output contains `FE EF AA 55`, a u16LE envelope length, command identifier, optional u32LE packet number, logical opcode/data, and a u16LE checksum. It does not contain a nested classic packet. Page146's length wording omits the identifier, while the captured brightness fixture and reference implementation count it. Preserve that discrepancy in tests and documentation. The captured acknowledged brightness packet is `feefaa550900010100000074007f00`.

The reference's u16BE sequence `0x0101..0x01ff` followed by three zero bytes is the same observed byte layout as identifier `1` plus a u32LE packet number in `1..255`. Model these fields directly. BLE notifications contain classic replies and can fragment or concatenate them. Sequence acknowledgment `0x33` and application reply matching are independent conditions and may arrive in either order.

## Session and transaction ownership

```rust
pub trait Transport {
    type Error: std::error::Error + Send + Sync + 'static;
    fn capabilities(&self) -> TransportCapabilities;
    async fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), Self::Error>;
    async fn read(&mut self, output: &mut [u8], deadline: Instant) -> Result<usize, Self::Error>;
    async fn close(self) -> Result<(), Self::Error>;
}

pub struct Session<T> { /* owns T, profile, decoder, bounded replies, BLE sequence, health */ }
impl<T: Transport> Session<T> {
    pub fn new(transport: T, profile: DeviceProfile, limits: SessionLimits) -> Self;
    pub async fn execute<R: Request>(&mut self, request: R, deadline: Instant)
        -> Result<R::Response, SessionError<T::Error>>;
    pub async fn receive(&mut self, deadline: Instant) -> Result<Option<Event>, SessionError<T::Error>>;
    pub async fn barrier(&mut self, deadline: Instant) -> Result<DeliveryReceipt, SessionError<T::Error>>;
    pub async fn save_indexed(&mut self, slot: IndexedSlot, bytes: &EncodedAnimation, deadline: Instant)
        -> Result<SaveOutcome, SessionError<T::Error>>;
    pub async fn upload_animation(&mut self, bytes: &EncodedAnimation, deadline: Instant)
        -> Result<TransferOutcome, SessionError<T::Error>>;
    pub async fn close(self) -> Result<(), SessionError<T::Error>>;
}
```

`Session` is not cloneable and exposes no transport accessor while connected. Every operation requires `&mut self`. The entire transfer, including start, all chunks, retransmission service, and completion, holds that borrow. Per-packet mutexes cannot protect this protocol because another command between chunks changes device state. Applications needing many producers may put one Session behind their own channel worker.

The transfer state is an enum with `Starting`, `Sending`, `Finishing`, and `Completed`, plus the saved retransmittable data. Cancellation or timeout after a write marks the session as having an interrupted operation. Reuse requires explicit recovery or reconnect, because host future cancellation cannot undo device state. Do not retry non-idempotent commands after an ambiguous write. BLE acknowledgment retry reuses the same packet number to benefit from device duplicate suppression.

The event queue retains unrelated valid replies. It has an explicit capacity and an overflow error. Avoid the reference transport's clearing of pending replies before sends. One absolute deadline bounds the operation, and per-write deadlines use its remaining time. Transport reads return EOF distinctly from timeout.

## Storage workflow contracts

`0x8b` is volatile animation transfer. Start is control0 plus u32LE total bytes. A response includes control0 plus readiness flag, which can refuse. Data is control1 plus total bytes, u16LE chunk index, and at most256 bytes. Finish is control2.

`0x8c` adds a zero-based destination byte only to start. It can start with a `0xb1` acknowledgment. Retransmission requests use `0x8c`, acknowledgment0x55, control1, and a requested u16LE chunk index. A repeated valid request resends the same bytes within retry limits. An out-of-range index is a protocol error.

The Max observation in `.audit/official-storage-reference.md` uses native AA streams with Max05/06 prefix frames. Its start reply is `01 05 00 04 B1 55 0F 01 02`. Completion can be a `0x46` reply whose mode is custom05, without a second `0xb1`. `SaveCompletion` represents `AcknowledgedByB1`, `ModeChangedToCustom(StateReply)`, or `UnconfirmedAfterTermination`. An unconfirmed completion is preserved as uncertainty rather than fabricated success or unconditional failure.

`0xb1` saves complete encoded frames with control1, u16LE current length, and image data. Start/end encode a typed artwork category. `0xb6ff` is a count query, while every other `0xb6` argument is a one-based deletion. On this Max the count is zero despite successful saves, so it cannot validate indexed upload. `0x8d` large-canvas transfer and `0x8e` 64-image enumeration remain separate request types with capability checks.

## Media contracts and gaps

```rust
pub struct Image { size: CanvasSize, pixels: Vec<Rgb> }
pub struct TimedFrame { image: Image, delay: FrameDelay }
pub struct Animation { frames: NonEmptyVec<TimedFrame> }
pub struct EncodedAnimation { format: NativeFormat, bytes: Vec<u8> }
pub enum NativeFormat { Palette16, PixooMax32, LargeCanvas64 }
pub fn encode(animation: &Animation, format: NativeFormat) -> Result<EncodedAnimation, MediaError>;
pub fn decode(bytes: &[u8], format: NativeFormat, limits: MediaLimits) -> Result<Animation, MediaError>;
```

The Go encoder establishes first-seen palette ordering, straight alpha channels, alpha<=32 mapped to black, minimum one bit per palette index, little-endian bit packing, zero count as maximum palette size, Max u16 palette counts with flag03, and AA length including marker and size bytes. Its Max animation prefix has flag05 and flag06 frames. These are compatibility fixtures, not interchangeable with 16x16 palette-reuse frames.

The Rust reference decodes only16x16, represents every nonzero palette flag as reuse, uses floating point to compute bit width, can index a palette without checking an encoded index, and treats truncated trailing frames as clean EOF. Those are unsuitable foundations for a complete checked decoder. Rebuild the codec around explicit frame variants and bounded subreaders.

Official image and animation pages refer to a native encoder function rather than publishing the full file format. LargeCanvas64 encoding, audio compression, downloadable font resources, alternate movie encodings, and some firmware resource bodies need separate evidence. A complete typed payload builder must not imply a complete encoder for an undocumented binary asset. Accept checked opaque asset bytes for those commands and label the codec gap in the generated catalogue.

The authoritative page corpus should distinguish documented requests, implemented payload encoders, implemented typed replies, fixture coverage, and hardware observations. Every official Bluetooth page maps to one owned family or an explicit non-command reference page. The Android catalogue is useful discovery evidence but cannot replace coverage of the official pages.

## Response decoding checklist

Decode generic replies before command-specific fields. Preserve unknown acknowledgment bytes as a rejected status rather than treating every non0x55 value as one undifferentiated boolean. Reject short matching replies. Preserve additional known-compatible bytes in a raw tail when the official response is extensible. Require exact lengths for scalar responses. Extended selectors match before the domain parser runs. Storage reply alternatives belong to transfer state, not a generic assumption that replies echo the outgoing opcode.

The highest priority typed response codecs are volume, current display state, firmware/version lists, schedules and alarms, storage readiness/retransmission/completion, saved item enumeration/count, track listing pagination, and BLE sequence acknowledgments. Where a page supplies no reply layout, expose `OwnedReply` alongside a named request instead of guessing fields.

## Rationale and synthesis input

Type System Discipline changes the API by separating index bases, units, validated ranges, and unknown response discriminants. Separate Before Serializing Shared State changes ownership by placing all protocol state behind one noncloneable Session and one mutable transfer borrow.

Compared with one giant command enum and a global encoder switch, this design places payload and reply logic together for the contributor editing one feature. The public Request trait is small enough to use for tests and pure encoding. The cost is one struct per distinct request and careful consistency of retry metadata. A private registry macro can derive boilerplate and catalogue entries without generating the substantive parsers.

The main open evidence gaps are the exact official escaping substitution table, the identifier-inclusive BLE length discrepancy,64x64 native assets, and completion behavior across models. They must remain visible in tests and support metadata. Max framing and Max storage behavior have explicit captured evidence and should be first-class profiles.
