# Command enum design for the Rust Bluetooth library

## Problem and evidence

The consumer needs a Rust library that covers the official Bluetooth documentation without inheriting the reference program's Linux, CLI, USB, Lua, or firmware assumptions. The library must preserve the difference between a documented packet layout and a behavior observed on a Pixoo Max.

I inspected `.audit/rust-reference/src/protocol`, its native media decoder, `src/transport.rs`, `src/lib.rs`, `src/control.rs`, `vendor/go-divoom/{frame,encode,profile,device,transport*}.go`, `.audit/official-146.json`, and `.audit/official-storage-reference.md`. The parent is collecting `rust/docs/official/<page_id>.json`, with 92 pages comprising framing and 91 command pages. This runner did not inspect that complete corpus. This sketch defines ownership and the required coverage mechanism. It does not claim that its illustrative command variants enumerate every page.

## Usage, derived before the types

```rust
use divoom::{Command, DeviceProfile, Percent, Session};
use divoom::transport::{Classic, ClassicOptions};

let profile = DeviceProfile::pixoo_max();
let link = Classic::connect(address, ClassicOptions::default()).await?;
let mut device = Session::new(link, profile);
device.execute(Command::SetBrightness(Percent::new(40)?)).await?;
let state = device.execute(Command::GetDisplayState).await?.display_state()?;

let native = divoom::media::encode_animation(&frames, profile.media())?;
let outcome = device.execute(Command::SaveIndexed {
	slot: StorageSlot::new(1),
	animation: native,
}).await?.transfer()?;
assert!(outcome.completion.is_confirmed());
device.finish().await?;
```

Pure callers can inspect command bytes without connecting. They choose an explicit dialect. No packet constructor accepts an arbitrary unchecked length.

```rust
let command = Command::SetBrightness(Percent::new(40)?);
let bytes = codec::encode_command(&command, Dialect::DocumentedClassic)?;
let mut decoder = codec::Decoder::new(Dialect::PixooMaxClassic, DecodeLimits::default());
let events = decoder.feed(&captured_bytes)?;
```

`codec` is public because pure codec use is a requested capability. Backend library types, OS handles, reader queues, BLE characteristics, and transport leases stay private.

## Shape and ownership

The organizing data shape is `Command` plus typed command arguments and `Reply`. The enum owns the exhaustive inventory. A sealed internal `CommandSpec` associates each variant with its opcode, payload encoder, reply rule, capability requirement, retry class, and source page. A single registry macro may generate those associations and `CommandKind`; there must not be a second hand-maintained opcode catalogue.

```rust
#[non_exhaustive]
pub enum Command {
	SetBrightness(Percent),
	GetDisplayState,
	SetDateTime(DeviceDateTime),
	SetAlarm(Alarm),
	SetDisplayMode(DisplayMode),
	SelectStoredTab(StoredTab),
	PlayAnimation(NativeAnimation),
	SaveIndexed { slot: StorageSlot, animation: NativeAnimation },
	SaveUserGif { kind: UserGifKind, animation: NativeAnimation },
	GetStoredCount,
	DeleteStoredItem(StoredItemNumber),
	Extended(ExtendedCommand),
	// Add the remaining documented variants from the 91-page manifest.
}

#[non_exhaustive]
pub enum Reply {
	Acknowledged(Delivery),
	DisplayState(DisplayState),
	Volume(Percent),
	StoredCount(u8),
	Transfer(TransferOutcome),
	// A typed response for every documented query.
}

pub struct Percent(u8);                     // 0..=100, checked constructor.
pub struct StoredTab(u8);                   // Consumer-facing 1..=3 for Max.
pub struct StorageSlot(u8);                 // Protocol zero-based destination.
pub struct StoredItemNumber(u8);            // One-based delete index, excludes 0xff.
pub struct ChunkId(u16);
pub struct FrameDuration(u16);              // Checked millisecond conversion.
pub struct DeviceDateTime { /* checked date and time */ }
pub struct Weekdays(u8);                    // Reject bit 7.
pub enum AlarmTrigger { Music(Frequency), Gif(GifId) }
pub struct Alarm { /* checked index, time, repeat, trigger, volume */ }
```

All constrained types have private fields, checked constructors, and accessors. Do not make all small integers a generic `Bounded` public API. Semantic names prevent mixing one-based delete indices and zero-based save slots. Unknown received enum values use explicit `Unknown(u8)` where lossless parsing is needed. Outgoing documented values remain checked.

The `Command` enum trades compile-time association between a command and its result for one discoverable, serializable command model. `Reply` accessors return a typed mismatch error. An alternative generic `Request<Response = R>` provides stronger result typing, but a heterogeneous automation queue then needs a second erased command representation. This candidate prefers one representation. If typed result association is a hard requirement, choose the generic request design throughout rather than supporting both indefinitely.

| Module | Knowledge and state it owns |
| --- | --- |
| `command` | Typed arguments, payload layouts, extended selectors, opcode registry, documented reply layouts, source metadata. Organize private files by documented feature families. |
| `profile` | Immutable framing dialect, media limits, capability declarations, observed reply alternatives, pacing defaults. Model identity is separate from a name string. |
| `codec` | Checked Classic encoding, escaping, checksum policies, bounded incremental stream parsing, response and event decoding, BLE envelope encoding and decoding. Pure functions only. |
| `media` | Checked pixels and dimensions, palette construction, native AA streams, palette reuse, flags, bit packing, animation timing. Optional image and GIF adapters accept bytes or image values, never paths as the core API. |
| `session` | One link, receive buffer, bounded event queue, BLE sequence, pending exchange, deadline, delivery evidence, and transfer transaction. |
| `transport` | Byte delivery and notification boundaries, deadlines, connect and close, negotiated write capacity, OS resource ownership. No command matching. |
| `error` | Public typed validation, codec, protocol rejection, timeout, disconnected, unsupported capability, and transport errors. Preserve error sources. |

`raw` can be an explicitly named expert module for unknown future commands. A `RawCommand` requires opcode, bounded payload, and a declared reply policy. It does not count toward typed coverage. Do not silently represent an incompletely implemented documented command as raw.

## Codec contracts

```rust
pub enum Dialect { DocumentedClassic, PixooMaxClassic }
pub fn encode_command(command: &Command, dialect: Dialect) -> Result<Vec<u8>, EncodeError>;
pub struct Decoder { /* dialect, bounded partial buffer, parser state */ }
impl Decoder {
	pub fn new(dialect: Dialect, limits: DecodeLimits) -> Self;
	pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Inbound>, DecodeError>;
	pub fn finish(&mut self) -> Result<(), DecodeError>;
}
pub enum Inbound { Reply(DeviceReply), Event(DeviceEvent), TransportAck(BleAck) }
```

The packet length counts the two length bytes, opcode, and payload. It excludes delimiters and checksum. Encoding checks `usize` arithmetic and `u16::try_from` before allocation. Sum the unescaped body. Apply the chosen checksum policy before byte stuffing. Escape only the interior, including checksum bytes, never the delimiters. Decoder reverses stuffing before length and checksum checks. Reject dangling escapes, overlong frames, impossible declared lengths, and truncated final input. Preserve fragmentation and coalescing. Do not split unescaped Max traffic by scanning for `0x02`.

`DocumentedClassic` uses the documented two-byte sum behavior and escaping. Page 146 says reserved bytes must be escaped but the inspected text does not define the mapping. The implementer must cite the actual mapping from the complete corpus or label a supplied mapping as a compatibility convention. Do not present a guessed `03 xx` mapping as official evidence.

`PixooMaxClassic` is the observed unescaped dialect. It sums into `u32`; checksum length is two bytes below `0xffff`, four bytes at and above `0xffff`, exactly as `vendor/go-divoom/frame.go`. Parsing can calculate the checksum width from the declared body. It must test equality at `0xfffe`, `0xffff`, and `0x10000`. Checksum width is independent of length-field width. Keep receive policy explicit if captured device responses diverge from outgoing packets.

BLE envelopes form a separate codec, not a Classic frame wrapped blindly in another header. Page 146 describes `FE EF AA 55`, LE length, ACK identifier, optional LE `u32` packet number, data, and LE checksum. The length excludes the command identifier according to the inspected prose, although checksum includes it. Require golden tests for both acknowledged and unacknowledged envelopes. Preserve that apparently awkward counting rule explicitly.

The reference BLE layout uses a two-byte big-endian sequence followed by three zeros. At values `0x0101..0x01ff`, those bytes equal identifier `1` plus a four-byte LE packet number `1..255`. Model the latter directly. Do not generalize the reference's byte-order trick to a full `u16` sequence. Distinguish BLE transport ACK `0x33` from command ACK. Session retransmission reuses the same envelope ID, while the next logical command advances the ID. ATT fragmentation uses the negotiated write capacity and write mode. It never changes the protocol length or sequence.

## Media contracts

```rust
pub enum Canvas { Square16, Square32, Square64 /* only supported codecs */ }
pub struct Rgb8 { pub r: u8, pub g: u8, pub b: u8 }
pub struct PixelFrame { /* checked canvas, exact pixel count, duration */ }
pub struct NativeAnimation { /* validated AA bytes, canvas, frame count */ }
pub fn encode_animation(frames: &[PixelFrame], profile: MediaProfile)
	-> Result<NativeAnimation, MediaError>;
pub fn decode_animation(bytes: &[u8], limits: MediaLimits)
	-> Result<Animation, MediaError>;
```

The core accepts exact pixels and never silently resizes. Optional adapters expose an explicit resize and alpha policy. Reproduce the Go first-seen palette and least-significant-bit packing where compatibility requires them. A one-color palette still uses one bit per pixel. Validate palette index bounds and frame lengths. For Max 32x32 output, emit flag frames 05 and 06 before data flag 03 and use a `u16` color count. The 16x16 zero count means 256 colors when a palette is replaced. Retain palette-reuse semantics in decoding.

Every frame and animation counter uses checked conversion. `decode_animation` accepts EOF only between frames. EOF inside a palette, header, or packed pixels is an error. Native frame format is inferred from references because official pages 289 and 290 delegate to `divoom_image_encode_encode_pic` without a full binary layout. Label that evidence. Do not infer 64x64 encoding from the existence of 0x8D commands.

## Session contracts and transfer state

```rust
pub struct Session<L> { /* sole mutable owner of link and protocol state */ }
impl<L: Link> Session<L> {
	pub fn new(link: L, profile: DeviceProfile) -> Self;
	pub async fn execute(&mut self, command: Command) -> Result<Reply, Error>;
	pub async fn next_event(&mut self, timeout: Duration) -> Result<Option<DeviceEvent>, Error>;
	pub async fn finish(self) -> Result<(), Error>;
}
pub enum Delivery { Written, TransportAcknowledged, CommandAcknowledged }
pub enum CompletionEvidence { CommandReply, DisplayState(DisplayState), Unconfirmed }
pub struct TransferOutcome { pub completion: CompletionEvidence, /* bytes and retries */ }
```

Exclusive `&mut self` covers the complete operation, including uploads. A consumer can put the session in a mutex if sharing is necessary. Individual packet locks are insufficient. There is no cloneable session, global selected transport, task-local configuration, or independently callable `send` and `receive` pair that bypasses transaction ownership.

The session's private transfer state is `Starting`, `Sending`, `Finishing`, `Complete`, or `Failed`. The transfer plan knows its stream, chunk size, total byte count, next chunk, retransmission limit, and response rules. Encode before the first write so a bad late frame cannot leave a partial upload. While waiting, route unrelated messages into the bounded event queue. Never clear notification queues before each packet. Unexpected disconnect and queue overflow produce typed errors with stage and delivery evidence.

For 0x8B, readiness is `04 8B 55 00 01`; refusal is the explicit readiness value zero. Data chunks carry total `u32LE`, zero-based `u16LE` ID, and at most 256 bytes. Termination has control 2 only. For 0x8C, start adds the destination slot, readiness may arrive as B1, and retransmission `04 8C 55 01 <idLE>` requests a specific chunk. The session handles retransmission while it owns the transaction.

For 0xB1, start and end carry the image-kind value and each data message carries the complete frame with its checked `u16` length. This is a different transfer plan from 0x8C. Do not squeeze both into a struct with unused fields.

Pixoo Max 0x8C completion accepts the observed `0x46` custom-mode response after termination through a named profile rule. It does not wait forever for a second B1 ACK. A B6 count of zero is not evidence of failure for this path. A transfer timeout after sending termination returns stage and unconfirmed outcome; it never reports that the save definitely failed. No generic automatic retry repeats destructive storage commands. Read-only probes may retry under an explicit bounded policy.

## Platform transport contracts

`Link` is an asynchronous transport contract with bounded `write_all`, `read`, and `close`, plus kind and maximum write capacity. Its exact future syntax should match the chosen stable Rust version. It makes no protocol decisions. For a stream, `read` may return any partial segment. For GATT, it returns a notification segment. EOF and timeout are distinct. A synchronous `Read + Write` adapter without enforceable deadlines cannot claim bounded session transactions.

Implement real Windows AF_BTH RFCOMM and Linux AF_BLUETOOTH RFCOMM behind target-specific modules. Windows initialization and socket ownership use RAII. Both backends handle partial writes, zero progress, cancellation, and read deadlines. RFCOMM channel is a checked 1..=30 type. A serial adapter is explicitly named and does not masquerade as native Bluetooth discovery.

BLE should use a backend that actually supports the relevant platform, with GATT service `49535343-fe7d-4ae5-8fa9-9fafd205e455`, write `49535343-8841-43f4-a8d4-ecbe34729bb3`, and notify `49535343-1e4d-4bd9-ba61-23c647249616`. Device identifiers may be platform opaque values. Do not require every macOS BLE device to have a MAC address.

The supplied Rust transport is Linux BlueZ only. macOS RFCOMM requires an IOBluetooth bridge and an explicit main-run-loop integration, as the Go backend documents. A library must not silently take over the application's main thread. If no tested bridge is implemented, report `UnsupportedPlatform` and document serial attachment separately. Compile support, fake-link tests, and hardware validation are separate support claims. Do not claim native macOS RFCOMM merely because the crate compiles there.

Connect must not change the clock by default. BLE SetUTC initialization in the reference is observable mutation and belongs in explicit connect options if required by a documented device profile. A read-only probe and sequence primer may be profile-owned, with their limitations visible.

## Reference flaws this design avoids

- Rust `Packet::serialize` truncates lengths with `as u16`, wraps the checksum, and has no escaping or Max variable checksum policy.
- Rust `Response::deserialize` accepts a seven-byte minimum although it reads reply fields and a two-byte checksum. A genuine response with opcode and ACK requires at least nine bytes under that framing.
- The Rust stream reader trusts the length for allocation and assumes a two-byte checksum. It cannot safely decode Max variable-checksum traffic or escaped frames.
- Rust media decoding indexes palettes without bounds checks, ignores the declared frame length, and treats any `UnexpectedEof` as successful animation termination. Some non-EOF I/O errors can continue its loop without progress.
- Rust one-color `bits_per_pixel` is zero, while the Go encoder forces one. Media needs an explicit compatibility decision and fixtures.
- The Rust animation transfer struct permits a start packet with unused arbitrary image bytes and a data packet over 256 bytes. A transfer enum or private transfer planner removes those invalid states.
- Rust transport selection is ambient state. Every connection independently owns reply queues, discards pending replies, and assumes reply opcode equals sent opcode unless its callers know the exception.
- The reference BLE connect changes the clock and hardcodes sequence behavior. Its wire layout can mislead a reader about the documented packet-number width.
- Go `isCompleteFrame` accepts any buffer ending in 02. Its barriers do not validate length, checksum, reply opcode, or ACK, and its writes do not handle partial success.
- The Go profile is mutable exported field data and its encoders cast counters without bounds checks. Its 200-byte legacy 0x49 chunking is not the documented 256-byte 0x8B or 0x8C transfer format.

## Coverage and verification contract

Store the official page snapshots with a manifest containing all 92 page IDs, titles, content hashes, category paths, source URLs, and a disposition. Each of the 91 command pages must point to typed command variants, payload encode tests, response decode tests, and any transfer-state tests. Introductory and media-delegation pages have explicit dispositions. A manifest check fails for a missing page, an unmapped command, or a typed implementation whose source page is absent. Counts alone do not establish coverage.

Use fixtures derived from official examples and captured transcripts, with provenance kept beside each. Test reserved bytes in the length, payload, and checksum; checksum widening equality; every possible fragmentation boundary; concatenation; bounded corrupt inputs; one-color and maximum palettes; truncated native frames; BLE ACK and command response in either order; storage alternative replies; retransmission; timeout after termination; and unrelated notifications during transfer. Use a fake link to prove the complete transaction bytes and outcome. Target compile checks do not replace native transport integration tests.

The first implementation unit should establish checked domain types and both Classic codec dialects with captured fixtures. The next unit adds the complete official command registry and manifest before platform code can obscure gaps.

## Tradeoffs and red-flag screen

The enum keeps opcode and reply behavior discoverable at the cost of runtime `Reply` matching. It hides multi-packet transfers behind one call. Backend details never enter command arguments. Session owns all mutable protocol state. Public codec access is deliberate expert functionality with a checked typed input, rather than importable OS internals. The page manifest is independent evidence and is checked against the registry, not a second catalogue to synchronize by hand.

Model the Domain shaped the command enum and transfer state machine. Type System Discipline shaped the separate index types and checked constructors. Both leaf skills were read for this sketch. The design is a candidate for the parent's synthesis, not an implemented or independently validated final design.
