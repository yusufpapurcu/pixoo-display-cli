# Framing and transaction ownership

`Packet` contains a validated opcode and payload. Classic encoding writes `01`, a two-byte length, opcode, payload, checksum, and `02`. The length counts itself, opcode, and payload. The checksum sums those unescaped bytes.

`ClassicDialect::Escaped` uses a wrapping 16-bit sum and the historical escape convention for interior bytes 01 through03, including checksum bytes. `Unescaped` uses the same sum without stuffing. `PixooMax` uses no stuffing and widens the checksum to u32 at sums greater than or equal to0xffff. That exact equality is covered by a fixture. Incoming streams use declared length and computed checksum width; embedded delimiter bytes do not end a frame.

`Decoder::feed` retains partial frames and returns all complete packets. Invalid headers, lengths, escaping, checksums, and footers poison the decoder. `reset` explicitly starts a new stream. `finish` rejects truncated input. Declared bodies are bounded by u16 and an optional smaller limit. `Packet::reply` parses the acknowledged main04 wrapper. `Packet::notification` parses the separate wrapper without an ACK byte.

The BLE envelope starts with `FE EF AA 55`. It carries a length, identifier, optional u32LE packet number, logical opcode plus payload, and u16LE checksum. It does not contain a Classic frame. Page146's length wording excludes the identifier, while the inspected captured fixture includes it. `BleLength::Documented` and `IdentifierIncluded` preserve both conventions. BLE device notifications in the inspected implementation contain Classic response frames, so `Session` decodes them with the selected Classic dialect.

One Session owns the transport, incoming decoder, bounded event queues, BLE sequence, and transaction state. Each exchange drains complete prior input into events before writing. Partially received prior frames finish before a new write. Private epochs prevent queued replies from completing a later transfer. BLE ACKs must match the current u32 number and remain separate from application responses. Missing BLE ACKs retry the exact envelope within configured bounds. Retryable transport errors mean zero progress; ambiguous GATT errors abort the session.

Classic replies have no request identifier. An identical delayed reply that first arrives after a later identical command cannot be conclusively attributed. Exclusive ownership, priming, barriers, queued-event epochs, and conservative failure handling reduce this risk. They cannot add a sequence field to the wire protocol.

Every complete operation has one absolute deadline and a cancellation token. Reads and writes use bounded polling. Interruption after a write can leave device state uncertain, so the session becomes poisoned and requires reconnecting. Read-only startup priming is the only whole-command retry helper. A save transaction is never automatically restarted.

`Receipt::delivery` reports the evidence received. A write says bytes reached the transport. A BLE ACK confirms that transport packet number. An application reply confirms the documented response, and a status-less response has its own delivery value. A communication barrier confirms a subsequent status query. None of these receipts independently proves a physical display change or flash persistence.

The default matrix16, large64, and documented profiles return `TerminatorWritten` for indexed or64 storage where no final reply is specified. The Max profile waits for the observed custom-mode status. Legacy B1 end acknowledgment is an explicit option. Unconfirmed termination is a successful transmission outcome with limited evidence, not a persistence claim.
