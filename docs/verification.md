# Verification evidence

This repository distinguishes codec tests, transport compilation, device responses, and physically observed effects.

## Device observations

The Rust `read_status` example ran on Windows against the already paired Pixoo-Max on 2026-10-24. It loaded the existing local Go configuration, connected through native Winsock RFCOMM, primed with bounded46 queries, validated framing and checksum, decoded the complete status, and exited0. It sent no setter, storage write, reset, or firmware command. The returned record included mode6, system brightness100, temperature mode0, clock colorRGB255/80/0, and extension bytesA4/2C. Mode labels follow the official table; the meaning of mode6 on this particular Max firmware was not physically established.

The original Go production capture is retained in `tests/fixtures/max-save-capture.txt`. It contains two startup status queries, B1 readiness for8C,43 data chunks, termination, and46/custom-mode completion. The Rust session test reconstructs all10822 native bytes and reproduces all45 storage packets byte for byte. With media enabled, it also decodes the24 frames and re-encodes the exact original native stream. This is replay verification of an earlier physical transaction, not a new Rust storage write.

The owner previously confirmed recall from all three stored tabs after switching away and reconnecting. Those observations are documented in `max-storage-evidence.md`. No power-cycle retention claim is made. This implementation run left the Go tools, pairing configuration, controller processes, and saved artwork unchanged.

## Platform checks

| Platform and backend | Evidence |
| --- | --- |
| Windows Classic | Native backend compiles; the Rust read-only device query succeeded. |
| Windows BLE | All-feature build and hardware-free queue, deadline and runtime tests pass. No physical BLE test. |
| Linux Classic | `cargo check --target x86_64-unknown-linux-gnu` succeeds, including the native socket backend and media. No Linux Bluetooth hardware test. |
| macOS BLE | `cargo check --target aarch64-apple-darwin --all-features` succeeds. No macOS Bluetooth hardware test. |
| Linux BLE | btleplug backend is provided. Native Linux execution and D-Bus integration were not run on this Windows host. Linux CI installs libdbus development packages. |
| macOS Classic | No IOBluetooth backend. The library does not claim native Classic support on macOS. |

Cross-compilation checks type compatibility. They do not establish runtime permissions, radio availability, device initialization sequences, or firmware behavior. The checked-in CI workflow is prepared for Windows, Linux, and macOS; it has not been published or executed by GitHub in this local task.

## Reproduce the checks

The final Windows verification run passed81 unit/integration tests and3 Rustdoc examples with all features, plus64 unit/integration tests in the minimal-feature build. Clippy with warnings denied, Rustdoc with warnings denied, formatting, the offline media example,91-page source/fixture validation, and13 local documentation-link checks also passed.

From the Rust repository, run the commands in the README. The coverage generator validates91 command-page mappings,90 distinct base opcodes, source snapshot hashes, and fixture paths. The Rust coverage test also resolves mapped API symbols and fixture function names.

Protocol tests cover every fragmentation boundary of captured replies, coalesced packets, corrupt framing and checksums, escape substitutions, truncation, maximum packet sizes, the exact checksum-widening boundary, BLE identifiers and both length conventions. Deterministic adversarial byte streams check parser error paths.

Session tests cover partial writes, zero progress, retryable write readiness timeouts, refusal and negative ACKs, stale and unrelated responses, status-less responses, duplicate notifications, cancellation, EOF, absolute timeout, queue capacity, startup priming, BLE ACK order and retries, bounded transfer resends, complete B1/8B/8C/8D flows, playlist termination, and artwork pagination.

Media tests compare full wire messages with the independent Go corpus. They cover palette capacity and reuse, alpha policy, per-frame durations, GIF transparency and background/previous disposal, native truncation, counter bounds, and text positioning.

Independent review reports are retained under `.audit`. Review found and prompted fixes for stale transfer events, retry-barrier ordering, write timeout handling, queue boundaries, Windows connect error readiness, status-less wrappers, whole-list ownership, and unsupported final-ACK assumptions. The audit trail records the final dispositions. Independent review used the available parent model, so model diversity was reduced.
