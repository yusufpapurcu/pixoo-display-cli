# Sources and attribution

The public protocol belongs to Divoom. This library is an independent implementation for interoperability and is not affiliated with Divoom.

- The 92 JSON snapshots in `docs/official` were retrieved from the public [Divoom ShowDoc portal](https://docin.divoom-gz.com/web/#/5/146) on 2026-10-24. They contain the framing article and 91 Bluetooth command articles. The source text retains Divoom's rights. Each command snapshot has a SHA-256 digest in `docs/coverage.json`.
- [thedaneeffect/go-divoom](https://github.com/thedaneeffect/go-divoom), commit `4df90757bca25270f20486731c9193d95f1a1ff1`, supplies the independent golden corpus, Max framing behavior, native AA format, and Windows transport research. Its MIT license is reproduced in `licenses/go-divoom.txt`. The Go golden JSON is copied into `tests/fixtures/go-goldens.json`.
- The existing workspace's maintained Go storage tool supplies `tests/fixtures/max-save-capture.txt`. The transcript records a successful 24-frame, 10822-byte, 43-chunk Max indexed save. The owner confirmed reconnect recall for all three stored tabs. Power-cycle retention was not tested.
- [futpib/divoom-ditoo-pro-controller, rust branch](https://github.com/futpib/divoom-ditoo-pro-controller/tree/rust), commit `e1da802100934a9dacc600c5c7da093b0de61445`, was inspected for BLE fixtures, reply matching, palette reuse, and capability gaps. No license file was found in that checkout. Its implementation source was not copied. The captured BLE brightness fixture is cited in `tests/protocol.rs`.
- [hass-divoom issue17](https://github.com/d03n3rfr1tz3/hass-divoom/issues/17) contains the historical `03 04`, `03 05`, `03 06` escape substitution convention. Official page146 requires escaping without defining that substitution table. The `Escaped` dialect names this compatibility convention explicitly.
- Microsoft [Bluetooth sockets](https://learn.microsoft.com/en-us/windows/win32/bluetooth/bluetooth-and-socket) and [select](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-select) define the Windows socket backend. Linux uses the AF_BLUETOOTH RFCOMM socket ABI.
- The [GIF89a specification](https://giflib.sourceforge.net/gifstandard/GIF89a.html) defines GIF frame timing, transparency, and disposal.

Third-party Rust dependencies retain their own licenses. `Cargo.lock` records the versions used for verification.

The `pixo` CLI renders its bundled pixel font through the MIT-licensed `font8x8` crate, version 0.3.1 by Joaquin Rosales. Its license is reproduced in [licenses/font8x8.txt](licenses/font8x8.txt). The CLI uses printable ASCII glyphs from `BASIC_FONTS`; it does not supply arbitrary Unicode shaping or native Divoom word serialization.
