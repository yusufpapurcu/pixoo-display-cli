# Rust Bluetooth library implementation

- [x] Read the Principles section of poteto-mode.
- [x] Phase A: Frame.
- [x] Phase B: Design the workflow.
- [x] Phase C: Run the loop.
- [x] Phase D: Keep the audit trail.
- [x] Phase E: Verify and hand back.

## Designed execution units

- [x] Capture all official Bluetooth pages and a source manifest.
- [x] Ground, Sketch, Agree, Implement, Scrap. Compare two independent designs before implementation.
- [x] Frame, Fan out, Cross-judge, Pick, Graft, Verify for the architecture sketch.
- [x] Implement incremental Classic and BLE codecs with golden, malformed, fragmented and coalesced fixtures.
- [x] Implement every documented typed command and decode documented response structures.
- [x] Implement native media and per-frame timing, GIF compositing and text encodings supported by evidence.
- [x] Implement exclusive session workflows, storage and transfer retry bounds, timeout/cancellation and short-write handling.
- [x] Implement platform transport adapters, keeping unsupported hardware claims explicit.
- [x] Link all 91 pages to APIs, fixtures, device assumptions and verification evidence.
- [x] Run examples, tests, lint, documentation and independent review. Fix accepted findings.
- [x] Create local repository and commit. No publish, push or CLI design.

Done predicate. Every official Bluetooth command has a typed checked encoder and independently sourced fixture or an explicit source gap. All documented replies and transfer workflows have adversarial stream/session tests. All examples compile and tests/lint/docs pass. Native Max media matches Go goldens. The coverage matrix enumerates every page with no invented hardware claims. Source gaps stay visible and prevent claiming unsupported full hardware compatibility.

Throughput checkpoint. Root owns packet/session/transport integration and all hardware access. Independent design readers own separate output files. Subsequent implementation delegates own disjoint modules. Verification precedes each integration. No hardware setters are necessary to establish protocol coverage.

Architecture Scrap step. skip: the selected shape remained valid after review; fixes preserved its module ownership.
Opening a PR. skip: user explicitly requested a local dedicated repository with no public push or publication.
Cross-family review. skip: available reviewers inherited the parent model; further fresh dispatch reached the agent limit. Independent source review completed with reduced diversity.
