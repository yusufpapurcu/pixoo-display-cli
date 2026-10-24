# Completion review

**Verdict: PASS — no remaining proven correctness issue after the closure fix below.**

Reviewed immutable staged tree `983f7070ff8743cb456ae51b7c00bd95d4b79cb1` of the dedicated Rust repository, then inspected its closure delta in `src/session.rs` and `tests/session.rs` for the B1 fix below. This closes the prior reviews against the command corpus, protocol/session/media/transport source and regressions. No root source or tests were changed by this reviewer; no hardware was contacted. Root was updating documentation and rerunning final verification concurrently.

## Final source finding: fixed and verified

**Closed P2 — legacy B1 saving could return a success receipt after an explicit frame rejection.**

Source: `src/session.rs::Session::save_user_gif`, its frame loop and final exchange. Protocol source: `docs/official/294.json` defines B1 command responses with the acknowledged main04 wrapper; `Reply::ensure_success` establishes the library's rejection semantics for non55 replies.

Concrete former sequence: B1/start receives B1/55; a B1/frame write receives B1/00; final B1/end is silent. The frame exchange had no expected reply, so it returned after writing. The next exchange's preflight drain queued the negative B1 reply as an event. No save-level code inspected that queued rejection. The default completion policy returned `TransferCompletion::TerminatorWritten`, even though the owned transaction had already received an explicit failure. With `B1Completion::CommandAcknowledgment`, a later final B1/55 could similarly make the save return success while the earlier negative B1 remained queued.

This is distinct from requiring an undocumented final ACK. A terminator-written receipt properly leaves persistence uncertain, but a known failure of the current transaction should still return `Error::Rejected` and poison that interrupted save.

Root accepted the finding and added operation-scoped B1 validation after frame writes and before returning under either completion policy. Positive frame ACKs are consumed; stale earlier-operation B1 events remain outside the scope. The default written policy observes the configured quiet interval for an optional end rejection. This reviewer inspected the exact source/test delta and ran `cargo test --offline --test session b1_frame_rejection_aborts_before_end_for_both_completion_policies`: **1 passed**,23 filtered out. The regression verifies both completion policies return the B1 rejection, poison the session and abort before end. The fix closes this finding.

## Prior findings: closure

The following repairs are present and have targeted regressions:

- Old queued replies and retransmission requests cannot satisfy a later transfer. Epochs start after draining preexisting complete/partial input; completion is bounded by the latest terminator.
- Pacing occurs before preflight draining, so a reply arriving during pacing remains attributed to the previous operation. Retries retain exact BLE envelope bytes and packet number.
- Volatile retransmission writes a replacement terminator and obtains a fresh status barrier.
- Zero-progress retryable writes continue under the absolute deadline; ambiguous BLE writes instead fail nonretryably.
- Input draining checks cancellation/deadline between nonblocking reads, and valid exact-capacity queues work.
- EOF/fatal receive errors poison session health; Windows asynchronous connect failure readiness includes exception descriptors before SO_ERROR.
- Status-less13/14/17 wrappers preserve the first payload byte. Matching notification duplicates preserve the first response and queue subsequent notifications.
- Time-management Disable does not require the Get-only57 response.
- SD playlist collection owns records through14/end and handles an empty result. Artwork inventory owns8E pages through matching control2/end, checks totals/ranges, reconstructs out-of-order IDs and rejects incomplete/conflicting records. Generic single-response command execution rejects these stream commands before writing.
- Generic indexed/64 storage defaults to bounded quiet/resend handling and explicit `TerminatorWritten` evidence. Captured Max indexed saving still requires a full46/custom05 response. Legacy final B1 ACK is an explicit policy.
- The inherited full Max transcript is replayed byte for byte:45 storage packets,43 chunks,10822 native bytes and24 frames. This is historical capture replay, not a new storage write.

No additional confirmed payload-field, checksum, native AA16/32 encoding, GIF composition, palette, counter-width or platform socket ownership defect was found. Official field-width/encoding ambiguities remain explicit caller choices.

## Verification evidence and attention flags

The recorded verification transcript reports80 unit/integration tests plus3 documentation examples passing before the final small pacing relocation. This reviewer previously ran the all-features suite successfully at the earlier71-test checkpoint and independently verified the final B1 regression after inspection. Root was rerunning `tools/verify.ps1 -Offline` for the final source; use that final rerun, not the earlier transcript, as handback evidence. The closure verdict concerns inspected source correctness; the final validation/commit steps must still be completed by root.

The original staged tree omitted `docs/ble.md` despite links in README, the BLE module documentation and the trail. Root added that document during closure. Its working copy now explains runtime requirements, MTU/write bounds, subscriptions, connection ownership, cancellation ambiguity, cleanup and physical-test limits. Include it in the final staged/committed artifact. Its absence from the originally reviewed tree is a documentation attention item, not an additional remaining source fault.

The17:22 checkpoint expanded `decisions.tsv` into a useful retrospective trail, explicitly correcting the initial placeholder midnight timestamp instead of inventing exact earlier execution times. The implementation checklist now reflects completed implementation and evidence capture; final validation/handback and repository commit remained unchecked during review. Close those rows only after the final checks and actual local commit. Retain the original review reports as historical findings, with this closure report describing their disposition.

Explicit remaining limits must remain in the final user handoff:

- Native64 frame encoding is unresolved;64-pixel images may be loaded/resized, and externally encoded assets transported, but native encoding returns Unsupported.
- Native87 word-content serialization is unresolved; attribute APIs, structural text inputs and host bitmap rendering exist, while the proprietary word encoder requires supplied encoded bytes.
- macOS Classic/IOBluetooth is absent. Windows Classic has a recorded read-only physical check. Linux Classic and macOS BLE compilation do not establish hardware behavior; Linux BLE/D-Bus execution and physical BLE compatibility remain unverified.
- Generic terminator-written completion does not establish storage acceptance or persistence; Max power-cycle retention was not tested. Many audio/SD/alarm/game/sensor setters have fixture evidence only.
- Independent reviewers used the available parent model. The trail acknowledges reduced model diversity and that a cross-family review panel was not run.
- Prepared GitHub CI has not been published or executed. The repository remains local; publication/push is outside this handoff.

These are accurately disclosed scope/evidence limits, not reasons to fabricate codecs or hardware claims. Source review is PASS with these attention flags retained. Finish the final verification rerun, stage the closure fix and BLE guide, reconcile the trail/checklist, and create the requested local commit before declaring the entire task complete.
