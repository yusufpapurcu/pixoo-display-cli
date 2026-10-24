# divoom-bluetooth

A Rust library for the [official Divoom Bluetooth protocol](https://docin.divoom-gz.com/web/#/5/146), with the separate `pixo` CLI for Pixoo-Max daily workflows. The library implements checked payload APIs for all 91 command pages, incremental Classic framing, the BLE envelope, reply decoding, native 16/32 pixel media, and exclusive transfer sessions.

Build the CLI with `cargo build -p pixo-cli --release`, or install it with `cargo install --path crates/pixo-cli --locked`. Run `pixo --help` for commands. The [CLI guide](docs/cli.md) covers named scenes, text and image previews, direct device uploads, configuration import, JSON results, and completion limits. CLI dependencies remain in their own workspace member. Existing library consumers do not depend on argument parsing, fonts, configuration, or process locking.

Command coverage and device compatibility are separate. The [coverage matrix](docs/coverage.md) links every command page to its API, fixtures, device assumptions, and unresolved fields. Native 64 pixel encoding and native text-content serialization require encoder bytes from another source. Their proprietary encoder functions are not defined in the available documentation. [Protocol gaps](docs/protocol-gaps.md) records these limits.

```rust
use divoom_bluetooth::{commands::{system::SystemCommand, Percent}, protocol};

let command = SystemCommand::Brightness(Percent::new(50)?).packet()?;
let bytes = protocol::encode(&command, protocol::ClassicDialect::PixooMax)?;
assert_eq!(bytes, [1, 4, 0, 0x74, 50, 0xaa, 0, 2]);
# Ok::<(), divoom_bluetooth::Error>(())
```

Use a local path dependency when consuming a checkout.

```toml
[dependencies]
divoom-bluetooth = { path = "../rust" }
```

The `media` default feature supplies PNG, JPEG, BMP, WebP, GIF, palette encoding, and bitmap text rendering. `--no-default-features` retains all command builders, codecs, sessions, and native Classic transports. The optional `ble` feature adds GATT discovery and transport through btleplug. Linux BLE builds need the system D-Bus development packages. The crate declares Rust 1.88 or newer. Verification used Rust 1.93.1.

## Use a session

Windows and Linux expose native RFCOMM. Supply the paired device address and channel from your own configuration. BLE addresses and platform device IDs are selected explicitly. The library contains no paired-device address.

```rust,no_run
# #[cfg(any(windows,target_os="linux"))]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use std::time::Duration;
use divoom_bluetooth::{
    commands::display::{DisplayCommand, LightSettings},
    profile::DeviceProfile,
    session::{CancellationToken, Session, SessionOptions},
    transport::{BluetoothAddress, Rfcomm, RfcommChannel},
};

let address: BluetoothAddress = std::env::var("DIVOOM_ADDRESS")?.parse()?;
let link = Rfcomm::connect(address, RfcommChannel::default(), Duration::from_secs(5))?;
let mut session = Session::new(link, DeviceProfile::pixoo_max(), SessionOptions::default())?;
let cancel = CancellationToken::default();
session.prime(3, Duration::from_secs(1), &cancel)?;
let result = session.execute_command(&DisplayCommand::GetLightMode, Duration::from_secs(2), &cancel)?;
let state = LightSettings::decode(&result.reply.ok_or("missing status")?)?;
println!("brightness {}", state.system_brightness);
session.close()?;
# Ok::<(), Box<dyn std::error::Error>>(())
# }
# #[cfg(not(any(windows,target_os="linux")))] fn main() {}
```

The `read_status` example consumes the existing Go configuration and refuses to connect if its configured controller port is listening. From this repository, `cargo run --example read_status -- ../.config/go-divoom/config.json` sends only bounded status queries. Keep one connection owner per device. `Session` itself is not cloneable, and complete transactions require one mutable borrow.

`execute_command` uses command-owned response metadata. A receipt distinguishes a write, BLE transport acknowledgment, command acknowledgment, and a status-less response. `GetWorkMode` returns `receipt.notification`; decode it with `SystemReply::decode_event`. Unrelated replies and notifications remain available through `next_event`. SD playlists and artwork inventories use `sd_music_list` and `artwork_inventory`, which retain ownership through their end markers.

## Display and save media

`Frame::static_packet` prepares a static image. `Animation::packets` prepares a transient animation with an explicit `MediaProfile`. Pass the prepared packets to `Session::display_packets` to send the sequence and query a final communication barrier.

```rust,no_run
# #[cfg(feature="media")]
# mod example {
# use std::time::Duration;
# use divoom_bluetooth::{media::{Animation,Dimension}, session::{CancellationToken,FileDestination,IndexedSlot,Session},transport::Transport};
# fn save<T:Transport>(session:&mut Session<T>) -> Result<(),Box<dyn std::error::Error>> {
let animation = Animation::from_gif(std::fs::File::open("art.gif")?, Dimension::ThirtyTwo)?;
let native = animation.encode_native()?;
let saved = session.transfer_file(
    FileDestination::Indexed(IndexedSlot::new(0)),
    &native,
    Duration::from_secs(35),
    &CancellationToken::default(),
)?;
println!("{:?}", saved.completion);
# Ok(()) }
# }
# fn main() {}
```

`0x49` is transient playback. Indexed `0x8C`, volatile `0x8B`, large-canvas `0x8D`, and frame-based `0xB1` have separate workflows. The file transfer method validates all packet counters before starting, waits for readiness, handles bounded retransmission, and never repeats a whole save after an ambiguous failure. A native file is an AA frame stream, not a GIF file or outer `0x49` packets.

Max indexed saves use the captured `B1` readiness and `46/custom` completion. Other profiles default to `TerminatorWritten` where the documentation does not promise a final reply. This receipt leaves persistence unconfirmed. An explicit custom profile can require a known final ACK. `B1Completion` controls the corresponding legacy-save policy. A timeout or cancellation after transmission poisons the session; reconnect before further work.

See [native media](docs/media.md), [BLE transport](docs/ble.md), [framing and ownership](docs/protocol.md), [platform support](docs/verification.md), and [source attribution](NOTICE.md).

## Verify

Run `pwsh -File tools/verify.ps1` for the full check sequence. Add `-Offline` after dependencies are cached. The script also checks local documentation links and uses a workspace temporary directory for Rustdoc examples.

```text
cargo test --all-features
cargo test --no-default-features --all-targets
cargo clippy --all-features --all-targets -- -D warnings
cargo fmt --check
cargo doc --all-features --no-deps
pwsh -File tools/update-coverage.ps1 -Check
```

Tests include independent Go golden bytes, a complete captured Max save, every official command page, corrupt and fragmented streams, stale responses, short writes, cancellation, retry limits, whole-list end markers, GIF disposal, and per-frame timing. Hardware claims are limited to the [recorded evidence](docs/verification.md).
