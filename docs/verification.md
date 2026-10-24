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
| Linux BLE | btleplug backend is provided. Native Linux execution and D-Bus integration were not run on this Windows host. Building this backend requires the system D-Bus development packages. |
| macOS Classic | No IOBluetooth backend. The library does not claim native Classic support on macOS. |

Cross-compilation checks type compatibility. They do not establish runtime permissions, radio availability, device initialization sequences, or firmware behavior. Verification is run locally through `tools/verify.ps1`. This repository has no GitHub Actions workflow.

## Reproduce the checks

The current Windows workspace verification passed100 unit/integration tests with all features, comprising84 library tests and16 CLI tests, plus3 Rustdoc examples. The separate minimal-feature library build passed64 tests. Workspace Clippy and Rustdoc with warnings denied, formatting, the offline media example,91-page source/fixture validation, and18 local documentation-link checks also passed. The CLI also passes `cargo check -p pixo-cli --target x86_64-unknown-linux-gnu`; this is compilation evidence only.

The optimized Windows CLI passed16 additional external smoke checks through real subprocesses, configuration import, scene persistence, overwrite refusal, dry runs, and PNG/GIF previews. Pixel checks confirmed text rendering and centered contain padding. A dry run of the existing PR-merged GIF produced24 frames,10822 native bytes, and43 initial storage chunks, matching the inherited capture's asset metadata.

A read-only live CLI status attempt stopped at the conservative legacy-controller check because its localhost connection timed out. It returned exit3 with `ownership_check` and `delivery_uncertain: false`, before opening Bluetooth. This run does not establish a new physical CLI status, upload, or save result. Existing artwork, controller processes, and Go configuration were preserved.

From the Rust repository, run the commands in the README. The coverage generator validates91 command-page mappings,90 distinct base opcodes, source snapshot hashes, and fixture paths. The Rust coverage test also resolves mapped API symbols and fixture function names.

Protocol tests cover every fragmentation boundary of captured replies, coalesced packets, corrupt framing and checksums, escape substitutions, truncation, maximum packet sizes, the exact checksum-widening boundary, BLE identifiers and both length conventions. Deterministic adversarial byte streams check parser error paths.

Session tests cover partial writes, zero progress, retryable write readiness timeouts, refusal and negative ACKs, stale and unrelated responses, status-less responses, duplicate notifications, cancellation, EOF, absolute timeout, queue capacity, startup priming, BLE ACK order and retries, bounded transfer resends, complete B1/8B/8C/8D flows, playlist termination, and artwork pagination.

Media tests compare full wire messages with the independent Go corpus. They cover palette capacity and reuse, alpha policy, per-frame durations, GIF transparency and background/previous disposal, native truncation, counter bounds, and text positioning.

The earlier library review found and prompted fixes for stale transfer events, retry-barrier ordering, write timeout handling, queue boundaries, Windows connect error readiness, status-less wrappers, whole-list ownership, and unsupported final-ACK assumptions. That review used the available parent model, so model diversity was reduced.

CLI review prompted strict typed imports, bounded cover sampling, cancellation-aware connection results, and a narrower selected-device diagnostic claim. Regression tests reproduced invalid-import acceptance and the overly broad diagnostic contract before their fixes. Simulated transport tests cover composed updates, partial effects without replay, and cancellation before application I/O. A fresh review found no unresolved actionable findings. Review reports and decision logs remain in the gitignored `.audit` directory; checked-in tests, fixtures, and documentation retain reproducible evidence.
