# Final independent source review

This is a read-only review of root source. Reviewed all command enums, payload builders, typed reply decoders, media encoding/decoding/compositing, framing, session workflows, platform byte transports, and tests against all 92 saved official JSON pages (page146 protocol introduction plus91 command pages). Also checked the existing Go media fixtures, official storage evidence and captured Max save completion. No hardware was contacted and no root source/test file was modified. Root was actively addressing findings during this review; statuses below distinguish the observed source from subsequent fixes.

## Fixes from the first review

Source inspection confirms operation epochs now scope transfer retransmissions to their start and completion to the latest end; preexisting partial frames are consumed before assigning the next operation epoch. Volatile repair sends a new status query after its replacement terminator. Retryable zero-progress write errors use the remaining absolute deadline. Exact event-capacity input no longer immediately raises the unsettled-stream error. Fatal `next_event` receive errors now poison session health. Windows select now includes exception readiness before reading SO_ERROR for connect. The pacing duration subtraction saturates.

The regression tests exercise stale custom-mode completion, volatile repair ordering, exact queue capacity, zero-progress timeout retries, receive EOF poisoning, fragmented/coalesced replies, BLE ACK/reply ordering and byte-identical retries. No further confirmed Classic checksum or envelope-format defect was found.

## New concrete findings and follow-up checks

### 1. P2 — documented status-less replies were interpreted as rejected ACKs (fixed during review)

Sources: `commands/system.rs::SystemCommand::GetWorkMode`, `session.rs::Session::exchange/queue`, `protocol.rs::Packet::reply`; official [page179](../docs/official/179.json), [page207](../docs/official/207.json), [page280](../docs/official/280.json).

Page179 returns main04/cmd13/mode with no status byte. The old opcode-only expectation converted mode0..11 into `Reply.status`, rejected it as non55 and poisoned every valid documented work-mode query. Page280 game updates similarly had their game-type byte stripped as a status. A list-end notification04/14 has no ACK byte at all. Typed decoder tests constructed already-separated values and missed this integration error.

Root added `Notification`, `Packet::notification`, `Event::Notification`, notification-shaped reply expectations and `Command::response_is_notification`. The new wire-level regression `documented_statusless_work_mode_and_game_progress_keep_the_first_data_byte` passed in this review's all-features run. The resulting status distinction is supported directly by the source pages; it is not an invented ACK-success rule.

### 2. P2 — silent time-management disable was forced to acknowledge (fixed during review)

Sources: `commands/schedule.rs::ScheduleCommand::response_opcode`; official [page259](../docs/official/259.json).

Page259 distinguishes flag1 disabling from flag0 returning configuration status. The old matcher required57 for either control, so a silent successful disable timed out and poisoned the session. Root now requires57 only for `TimeManagementControl::Get`; the same new session test verifies that Disable returns Written without waiting for a reply.

### 3. P2 — input draining ignores cancellation and absolute deadlines (observed, reported to root)

Source: `session.rs::Session::drain_available` and its exchange/transfer callers. This conflicts with the session's timeout/cancellation contract, rather than a command page.

The revised drain accepts up to65536+event_capacity immediate reads, but checks neither its caller deadline nor cancellation between them. An isolated transport returning one byte at a time from a maximum-size valid Classic frame cancels the token during its first read; the session still performs over65500 reads before observing that cancellation. The isolated bug-characterization test `nonblocking_drain_ignores_cancellation_until_maximum_frame_finishes` passed and consumed roughly30ms locally even though cancellation occurred on read1. Real adapter/decoder overhead can extend a short deadline in the same way. Pass the absolute deadline and cancellation token into every draining loop and check between reads. Increasing a packet/read count bound does not impose the requested elapsed-time bound.

Reproduction command: `cargo test --manifest-path rust/.audit/session-review/Cargo.toml --offline nonblocking_drain_ignores_cancellation`. This test asserts the observed bug and should be replaced or inverted once fixed; do not interpret it as a regression proving correct cancellation.

### 4. P2 — coalesced matching notifications overwrite each other (observed immediately after wrapper fix)

Source: the notification-expectation branch of `session.rs::Session::exchange`; official [page179](../docs/official/179.json) establishes the status-less reply shape.

The new branch initially assigned `notification=Some(...)` unconditionally for each matching packet in pending. For GetWorkMode, return04/13/[3] then04/13/[4] in one read. The receipt contains4; the earlier response3 disappears and no later event remains. The ordinary acknowledged-reply branch already preserves its first response and queues duplicates. Apply the same rule to notifications: first match satisfies the command, later matches enter the bounded event queue. This was reported while root was editing the new branch.

### 5. P2 — a music-list request waits for an item rather than its documented completion

Sources: `commands/system.rs::SystemCommand::SdMusicList`, its `response_opcode`, `session.rs::execute_command`; official [page200](../docs/official/200.json) and [page207](../docs/official/207.json).

The request's expectation is opcode07. A nonempty playlist returns after the first song; remaining records and the terminal04/14 are left for callers to reconstruct manually. An empty playlist or valid range with no extant items can send only04/14, causing this command execution to time out and poison the session despite the documented completion arriving. A complete-list helper should own all records through14 under one deadline, accept an empty result, and return after that marker. Individual `SystemReply::SdName` parsing and `decode_event(14,...)` are useful pieces but do not implement this workflow.

Similarly, [page298](../docs/official/298.json) describes multiple8E/files responses through control2/end; `GetArtworkInfo` currently returns its first response. This is a declared workflow gap if command execution is intentionally single-response, and should not be advertised as whole-list orchestration. Tests should include zero records, several coalesced/fragmented pages, unrelated events, and the actual terminal marker. Root was informed and may add explicit helpers or narrow the claims.

### 6. P2 — undocumented final ACK policies can turn a completed transfer into a timeout

Sources: `profile.rs::{DeviceProfile::matrix16,DeviceProfile::documented,StorageCompletion}`, `session.rs::transfer_inner`; official [page296](../docs/official/296.json), [page297](../docs/official/297.json).

Page296 requires B1 readiness after indexed start; it does not define a mandatory secondB1 after end. Page297 likewise requires8D readiness after start and defines end as control2/no additional data, without a mandatory final8D. The built-in generic/matrix profiles require finalB1 for indexed saves, while `FileDestination::Big64` unconditionally requires final8D. A compliant device can reply to start, accept all chunks/end and remain silent afterward; the helper then reports Timeout and poisons the session.

Use an explicit unconfirmed/terminator-written completion policy where final acknowledgment lacks documentation/capture; the existing `TransferCompletion::TerminatorWritten` was unused in the reviewed source. Retain the observed46/custom05 completion for Max and allow an explicit caller-confirmed completion policy for devices with stronger evidence. This is the same distinction the existing Max storage capture demonstrates: acceptance/termination must not imply an invented ACK obligation. Root was informed during review.

## Coverage, evidence and media checks

- The saved92-page set includes all reviewed system, schedule, tool, display and storage commands. The request bytes and typed response field offsets I compared match their pages outside the issues above. Conflicting source indices and absent widths are deliberately exposed through explicit caller choices instead of silently guessed defaults.
- `coverage-system.json` and `coverage-display.json` preserve useful source gaps, but are hand-maintained descriptions. A row named after a fixture does not prove end-to-end session behavior: the status-less179 error and259 disable requirement both passed the earlier typed tests. Root is adding an executable coverage builder; it should verify file/API/test references and distinguish request codecs, reply codecs, workflows, media gaps and hardware evidence.
- Media frame construction, palette count widths, palette limits, LSB pixel packing, single-frame timing, Max32 flag containers, counter limits, GIF rectangle disposal and text bitmap clipping were inspected. Native64 and proprietary87 word serialization remain explicitly unsupported. No additional proven media correctness defect was identified. The Go-derived fixture provenance and license distinction are clearly documented; the unlicensed separate Rust reference is not presented as copied code.
- Runtime tests of notification overflow, partial reads, zero polls, nested-Tokio rejection and stream EOF passed. No Bluetooth connectivity, firmware acceptance, persistence or platform hardware behavior was inferred from mock tests. Windows failed-connect readiness was verified against Microsoft's official select contract in the first report; Linux RFCOMM was source-reviewed but not executed on this Windows host.

## Verification actually run

`cargo test --manifest-path rust/Cargo.toml --offline --all-features` completed successfully:71 tests (17 library,20 display,8 protocol,4 schedule,15 session,4 system,3 tools),0 failures; doc tests0. This build included root's status-less wrapper and time-management fixes. The only output warnings were Windows incremental-cache hard-link fallbacks. These results apply to that concurrent source snapshot, not to changes root makes afterward.

## Audit trail attention flags

At the last inspection, `.audit/decisions.tsv` contained only the initial frame row dated midnight with result `open`, despite implementation and several accepted correctness fixes. `.audit/implementation.todo.md` still showed all designed execution units and phasesB–E unchecked. Root explicitly said it was updating these; before handback reconcile both with the final actual validation, document source-gap decisions (especially missing final storage ACKs, unsupported64/text codecs, conditional replies, status-less wrappers and list completion), and record accepted/rejected findings with evidence and result. Do not claim the requested decision trail is complete while only its initial row exists.

The initial strict comment report remains in `session-review.md`. Root removed the exchange/retry narration during this pass. No additional comment-only application refactor was requested and no source comments were changed by this reviewer.
