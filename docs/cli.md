# Control a Pixoo-Max with pixo

`pixo` is a one-shot CLI for an already paired 32x32 Pixoo-Max over Classic Bluetooth. Windows and Linux have native RFCOMM transports. Only the Windows library status path has physical Rust verification. The save and tab-recall workflows follow inherited Max captures and owner observations. CLI tests use offline assets, subprocesses, locks, and a fake transport. They do not establish new physical device behavior.

The reusable library and CLI are separate Cargo packages. The CLI does not offer BLE, Classic discovery, a daemon, native64 encoding, native87 text content, or advanced audio/alarm/drawing controls. The underlying library retains those typed protocol APIs and their documented limitations.

## Build and configure

From the Rust repository:

```powershell
cargo build -p pixo-cli --release
.\target\release\pixo.exe --help
```

To install the binary on your Cargo executable path:

```powershell
cargo install --path crates/pixo-cli --locked
```

Pair the display through your operating system. Add its exact address without connecting:

```powershell
pixo device add desk --address AA:BB:CC:DD:EE:FF --channel 1
pixo device use desk
pixo doctor
```

The address is a placeholder. The first release accepts only profile `pixoo-max` and size 32. `device add` defaults to those values. A configured alias never silently overwrites another alias.

To import the parent workspace's Go configuration, run from that parent folder:

```powershell
pixo device import-go .config/go-divoom/config.json --name desk --profile pixoo-max --size 32
pixo device use desk
```

Import reads the original file without modifying it. Profile and resolution are explicit because the old configuration does not prove them. Import also preserves the local legacy-controller endpoint as a connection guard.

The versioned TOML configuration lives under `pixo/config.toml` in `%APPDATA%` on Windows, or `$XDG_CONFIG_HOME` and then `~/.config` elsewhere. An explicitly set `XDG_CONFIG_HOME` takes precedence on Windows too. `--config PATH` overrides `PIXO_CONFIG` and the default location. Device selection uses `--device NAME`, then `PIXO_DEVICE`, then `device use`'s saved default. A configuration can contain multiple aliases.

`device list` lists local configurations. `device capabilities` reports implementation evidence and limitations, not firmware detection. `doctor` validates the selected device's configuration. It does not validate other aliases or scene assets. Only `doctor --connect` opens Bluetooth for a bounded read-only status query.

## Show artwork and control the display

```powershell
pixo status
pixo show picture.png
pixo show animation.gif --brightness 35
pixo brightness
pixo brightness 30
pixo stored 2
pixo mode
pixo mode custom
pixo mode light --color '#FF5000' --brightness 40
pixo blank
```

`show` is transient playback. It returns after sending the artwork and receiving a final status reply. The device can keep playing after the process exits. GIF loop-count metadata is not represented in the native stream. There is no finite-loop promise.

System brightness and light-mode brightness are separate settings. `brightness VALUE` sends the system setter and checks the observed status value. `--brightness` on show, text, blank, stored, or scene play sets system brightness in the same connection before the display operation. If a later step fails, the error preserves any completed brightness step. Multiple changes in one invocation are not atomic.

`mode` reports the raw code and its documented interpretation. The physical meaning of mode 6 on the observed Max firmware remains unverified. `mode light --effect N` accepts the protocol's numeric effect selector; it does not invent effect names. `blank` uploads a black frame. It does not power off the device.

Nearest-neighbor `--fit contain` is the default for images and GIFs. It preserves aspect ratio and pads with black. `--fit cover` crops to fill the square; `--fit stretch` distorts to fill it. GIF disposal is composited before fitting. Input files are limited to 16 MiB. Still-image decoding is bounded to 4096x4096 and 64 MiB allocations; GIF canvases and frames retain the [library limits](media.md).

## Render text and offline previews

```powershell
pixo text 'PASS' --color green
pixo text 'BUILD PASSED' --overflow scroll --step 80ms
pixo text 'HELLO' --overflow wrap --align center
pixo text --file message.txt --overflow scroll
Get-Content message.txt -Raw | pixo text --stdin --overflow scroll
pixo text 'PASS' --color green --output pass.png
pixo text 'BUILD PASSED' --scroll --output scroll.gif
pixo show picture.png --output fitted.png
```

The bundled MIT-licensed 8x8 pixel font supports printable ASCII. Unsupported glyphs produce an error naming the code point. Text rendering happens on the host and uses normal native image or animation packets. It does not use the undocumented native87 word encoder.

Text accepts one literal, `--file`, or `--stdin`. Input is bounded to 4096 bytes. CRLF is normalized and trailing newlines are trimmed for piped input. Colors accept `#RRGGBB` or black, white, red, green, blue, yellow, orange, cyan, and magenta. Green is full-intensity RGB 0/255/0. `--background` defaults to black. `--align left|center|right` controls horizontal placement; text is vertically centered. `--line-spacing 0..8` adds rows between lines.

The default overflow policy rejects text exceeding the display. `--overflow clip` clips, `wrap` wraps at four 8-pixel glyph cells and rejects excessive height, and `scroll` renders one horizontal pixel step per frame. Scrolling requires one line. `--scroll` is shorthand for `--overflow scroll`. `--step` accepts 1..65535 milliseconds and defaults to 80ms. Duration inputs accept an `ms` or `s` suffix; bare numbers mean seconds. Explicit line breaks are supported for static text.

`--output` writes a local preview and opens no device or controller connection. A PNG requires one frame. A GIF can contain multiple frames and repeats indefinitely. Preview destinations must be new files; existing files are never overwritten. GIF previews quantize frame delays to centiseconds and colors to a 256-color palette. Native encoding retains the prepared RGB pixels and millisecond frame durations. Alpha values at most 32 become black; higher alpha preserves straight RGB. Previews normalize that policy to opaque pixels.

Offline previews and dry runs use the CLI's fixed Max 32-pixel target even without a configured device. They do not predict connectivity or physical display behavior.

## Save tabs and name scenes

```powershell
pixo save animation.gif --tab 1 --dry-run
pixo save animation.gif --tab 1 --replace
pixo stored 1
pixo scene add build-running --tab 2 --brightness 35
pixo scene add desk-default --file art/giza-night-twinkle-32.gif
pixo scene add passed --text 'PASS' --color green
pixo scene list
pixo scene show build-running
pixo scene play build-running
pixo scene play desk-default --brightness 30
pixo scene upload passed --tab 3 --replace
pixo scene remove passed
```

Tabs are numbered 1 through 3. Saving requires `--replace` because the device cannot reliably tell whether a tab is empty. Dry runs and previews do not require it. Saving does not send an additional recall command, although the Max transfer-completion workflow itself can enter custom mode. A completed transfer is not proof of power-cycle retention.

A scene is a local recipe referencing exactly one tab, file, or text value, with optional brightness. Adding, replacing, listing, showing, or removing a recipe changes no device state. Use `scene add NAME --replace ...` to edit an existing recipe. File scenes store absolute paths at creation; manually written relative paths resolve against the configuration directory. File and text content is validated when added and prepared again when played. A tab scene cannot be previewed or uploaded because the CLI cannot read its artwork back.

`scene upload` explicitly replaces a device tab with a file/text scene's native asset. Local names do not establish the current contents of a device tab. There is no storage inventory based on the Max's unreliable B6 count. Return to a known scene explicitly rather than expecting the CLI to restore arbitrary previous artwork.

## Inspect operations and consume results

```powershell
pixo show animation.gif --dry-run
pixo scene play build-running --dry-run
pixo --json status
pixo --device desk --json scene play build-running
pixo --timeout 30s --json show animation.gif
```

Dry runs describe prepared media, native byte count, initial chunk count, target tab where applicable, brightness changes, and expected completion policy. They never probe the legacy controller or open Bluetooth. Possible retransmissions and priming packets are not included in initial storage chunk counts. A dry run's `expected_completion` is not an observed result.

`--json` emits exactly one versioned object on stdout for handled success or failure. Help and version are separate modes. Diagnostics go to stderr. The envelope has `schema_version`, `command`, `device`, `ok`, and either `result` or `error`. Scene play/upload results include the scene name. Status preserves raw mode values and extension bytes. Errors include a stable category, `delivery_uncertain`, and `completed_steps` for partial composed changes.

Completion strings describe evidence. `response_received` means the requested status arrived. `communication_barrier` means the device answered a status query after the upload. `custom_mode_status` means the Max returned the captured custom-mode completion status. The generic library also distinguishes `terminator_written` and `command_acknowledged`; neither is a universal persistence guarantee. No result proves that a human saw the intended pixels.

| Exit | Meaning |
| --- | --- |
| 0 | Operation completed at its stated evidence level |
| 1 | Local I/O, signal setup, or unexpected process failure |
| 2 | Invalid input, media, or configuration |
| 3 | Device/configuration contention or inconclusive legacy-owner check |
| 4 | Unsupported platform or capability |
| 5 | Bluetooth connection failure |
| 6 | Device rejection, observed-result mismatch, or protocol failure |
| 7 | Device deadline expired |
| 130 | Cancellation |

## Connection ownership and interruption

Every online invocation prepares its assets, takes an OS lock keyed by normalized physical address, checks any configured legacy endpoint, connects, primes with bounded read-only queries, performs the complete operation, and closes. The lock is shared across aliases and configuration files. Configuration edits use a separate lock and reload before atomic replacement, avoiding stale-snapshot overwrites. Lock files remain on disk; holding the OS lock establishes ownership, not file existence.

The locks coordinate cooperating `pixo` processes only. They cannot exclude a phone or old Go tools. Close the phone app and release an existing controller's Bluetooth connection first. The CLI refuses a listening legacy endpoint and also refuses when that check is inconclusive. It never stops another process or silently uses the old HTTP daemon.

The device budget defaults to 10 seconds for controls/status and 60 seconds for media. Connection uses at most 5 seconds of that budget; priming uses at most three one-second attempts. Remaining work receives only the remaining budget. Offline decoding is outside the device timeout and has resource limits. Userspace time bounds depend on OS scheduling. Ctrl-C signals cancellation and cleanup is attempted. Cleanup warnings are separate from already completed device work.

An interrupted mutating operation may have changed the device. Such errors conservatively set `delivery_uncertain`. No entire save is automatically replayed or resumed. A later invocation starts a new connection. See [framing and ownership](protocol.md) and [evidence gaps](protocol-gaps.md) for the protocol's limits.

## Verify

Run `pwsh -File tools/verify.ps1 -Offline` after dependencies are cached. It checks the whole workspace while preserving a separate minimal-feature library run. CLI subprocess tests cover configuration import, scene round trips, previews, dry runs, JSON errors, missing glyphs, real lock contention, and guarded refusal before Bluetooth. A fake transport checks composed brightness/display success, partial failure, and absence of whole-operation replay. No test requires a physical device.
