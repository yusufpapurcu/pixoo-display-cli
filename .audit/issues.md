# Local protocol issues

These issues require evidence beyond the available official pages. No public issue was posted.

| ID | State | Scope and next evidence |
| --- | --- | --- |
| RUST-001 | Open | Native64 asset encoding. Obtain a licensed vendor encoder, format definition, or device/app capture defining frame flags and palette layout. Existing8D transport accepts encoded assets. |
| RUST-002 | Open | Native87 word serialization. Obtain `divoom_image_encode_frame_word` implementation or captures covering repeated glyphs, glyph matrices, positions and lengths. All seven attribute payloads exist. |
| RUST-003 | Open | macOS Classic. Implement IOBluetooth with caller-owned main-run-loop integration and test against real hardware. BLE is a separate backend and does not provide Classic on Max. |
| RUST-004 | Open | Drawing and64 ID layout ambiguities. Establish point-offset widths,5B/5C length widths,58 indexing, and8D FileId width for each target family. Explicit caller choices remain in APIs. |
| RUST-005 | Open | Cross-device hardware acceptance. Exercise only relevant setters and workflows on representative speaker, backpack and64 hardware. Protocol fixtures do not establish firmware support. |
| RUST-006 | Open | Max power-cycle retention. Existing owner observations establish reconnect recall for tabs1..3. A controlled power cycle remains untested. |
| RUST-007 | Resolved | Session review findings, status-less wrappers, end-marker collection, and known negative B1 replies. Fixes and regression cases are recorded in `.audit/completion-review.md`, `tests/session.rs`, and the decision trail. |

The full field-by-field limits and current evidence are in `docs/protocol-gaps.md` and `docs/coverage.md`.
