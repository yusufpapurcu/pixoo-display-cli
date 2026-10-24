# Design judgment

Scores are 1–5; 5 is strongest. These assess the sketches, not an implemented library.

| Criterion | A: global enum | B: typed requests | Proposed hybrid |
|---|---:|---:|---:|
| Full 91-page implementation without global duplication | 3 | 5 | 5 |
| Pure codec independent of transport | 5 | 5 | 5 |
| Exclusive transfer ownership and wrong-reply handling | 4 | 5 | 4 |
| Small, discoverable public API | 4 | 3 | 5 |
| Precise device and evidence differences | 5 | 5 | 5 |
| Total | 21/25 | 23/25 | 24/25 |

Recommend the hybrid: named command enums owned by domain modules, checked `.packet() -> Result<Packet>`, typed reply parsers, one noncloneable `Session` executing packets with explicit expectations, and dedicated typed methods that own complete transfers through `&mut self`.

A centralizes discovery but risks a global switch and runtime reply casts growing with all 91 pages. B best couples request and response, but its public trait, per-request types, reply policy, and retry hooks enlarge the interface. The hybrid preserves B's local ownership with less public machinery.

The hybrid's condition is that callers must not reconstruct command-specific expectations from opcodes. Define packet encoding, reply matching (including selectors and alternatives), retry safety, and page provenance together in each domain's authoritative declaration; derive catalogue coverage from those declarations. Keep the explicit Packet execution path as the codec/session boundary, without adding parallel generic Request and global Command APIs. Matching malformed replies must fail; unrelated replies must enter a bounded queue. Transfer methods retain ownership through readiness, replay, completion, and interruption recovery.

Keep profiles immutable and distinguish documented behavior, captured Max behavior, unsupported assets, and unverified hardware. Resolve the A/B disagreement about BLE length with sourced fixtures, preserving the discrepancy rather than choosing by design preference. Neither escaping mappings nor 64×64 native encoding may be inferred from incomplete evidence.

Red-flag result: avoid hand-synced global lists, split reply ownership, per-packet transfer locks, and shallow convenience wrappers. Public checked Packet access is justified by the required standalone codec; OS handles and transfer stages remain private.
