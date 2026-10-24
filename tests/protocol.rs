use divoom_bluetooth::{protocol::*, Error};

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn independent_go_and_max_capture_fixtures_at_every_boundary() {
    for s in [
        "010400740a820002",
        "01050004b1550f0102",
        "011b00044655050001ff5000640001036400ff5000020100000037086c0402",
    ] {
        let bytes = hex(s);
        for boundary in 0..=bytes.len() {
            let mut d = Decoder::new(ClassicDialect::PixooMax);
            let mut p = d.feed(&bytes[..boundary]).unwrap();
            p.extend(d.feed(&bytes[boundary..]).unwrap());
            assert_eq!(p.len(), 1);
            assert_eq!(encode(&p[0], ClassicDialect::PixooMax).unwrap(), bytes);
            d.finish().unwrap();
        }
    }
}

#[test]
fn reserved_bytes_in_length_payload_checksum_and_coalesced_frames() {
    for dialect in [
        ClassicDialect::PixooMax,
        ClassicDialect::Escaped,
        ClassicDialect::Unescaped,
    ] {
        let packet = Packet::new(0x49, vec![1, 2, 3, 0xff, 0, 1, 2, 3]).unwrap();
        let wire = encode(&packet, dialect).unwrap();
        let mut d = Decoder::new(dialect);
        let mut out = Vec::new();
        for b in wire.iter().chain(wire.iter()) {
            out.extend(d.feed(&[*b]).unwrap());
        }
        assert_eq!(out, vec![packet.clone(), packet]);
        d.finish().unwrap();
    }
    let p = Packet::new(0x74, vec![1, 2, 3]).unwrap();
    assert_eq!(
        encode(&p, ClassicDialect::Escaped).unwrap(),
        hex("01060074030403050306800002")
    );
}

#[test]
fn checksum_widening_includes_exact_ffff_boundary() {
    // Body length is 259 (03 01), opcode ff, 255 data bytes. Base sum is 259.
    for (sum, last, wide) in [
        (0xfffe, 250, false),
        (0xffff, 251, true),
        (0x10000, 252, true),
    ] {
        let mut payload = vec![255; 255];
        payload.push(last);
        let p = Packet::new(255, payload).unwrap();
        let wire = encode(&p, ClassicDialect::PixooMax).unwrap();
        let body_sum: u32 = wire[1..260].iter().map(|v| u32::from(*v)).sum();
        assert_eq!(body_sum, sum);
        assert_eq!(wire.len(), 259 + if wide { 6 } else { 4 });
        let mut d = Decoder::new(ClassicDialect::PixooMax);
        assert_eq!(d.feed(&wire).unwrap(), vec![p]);
    }
}

#[test]
fn invalid_checksum_footer_length_escape_and_truncation_are_rejected() {
    for bytes in [
        hex("0102007400760002"),
        hex("01030046000002"),
        hex("01030046490003"),
        hex("0103030702"),
    ] {
        let mut d = Decoder::new(ClassicDialect::Escaped);
        assert!(d.feed(&bytes).is_err());
        assert!(d.feed(&[]).is_err());
        d.reset();
        assert!(d.finish().is_ok());
    }
    let wire = encode(
        &Packet::new(0x49, vec![1, 2, 3]).unwrap(),
        ClassicDialect::PixooMax,
    )
    .unwrap();
    for stop in 1..wire.len() {
        let mut d = Decoder::new(ClassicDialect::PixooMax);
        d.feed(&wire[..stop]).unwrap();
        assert!(d.finish().is_err());
    }
    let mut d = Decoder::with_limit(ClassicDialect::Unescaped, 20);
    assert!(d.feed(&[1, 21, 0]).is_err());
    assert!(Packet::new(3, vec![]).is_err());
    assert!(Packet::new(4, vec![0; 65533]).is_err());
}

#[test]
fn generic_reply_and_transport_ack_are_distinct() {
    let p = Packet::new(4, vec![0x33, 0x55, 1, 0, 0, 0]).unwrap();
    assert_eq!(p.reply().unwrap().transport_ack().unwrap(), Some(1));
    assert!(matches!(
        Packet::new(4, vec![0x74, 0])
            .unwrap()
            .reply()
            .unwrap()
            .ensure_success(),
        Err(Error::Rejected {
            opcode: 0x74,
            status: 0
        })
    ));
    assert!(Packet::new(4, vec![0x74]).unwrap().reply().is_err());
    assert!(Packet::new(0x74, vec![]).unwrap().reply().is_err());
    assert!(Packet::new(4, vec![0x33, 0x55, 1])
        .unwrap()
        .reply()
        .unwrap()
        .transport_ack()
        .is_err());
}

#[test]
fn ble_captured_fixture_and_documented_length_variant() {
    let env = BleEnvelope::command(&Packet::new(0x74, vec![0]).unwrap(), Some(1));
    assert_eq!(
        env.encode(BleLength::IdentifierIncluded).unwrap(),
        hex("feefaa550900010100000074007f00")
    );
    assert_eq!(
        env.encode(BleLength::Documented).unwrap(),
        hex("feefaa550800010100000074007e00")
    );
    assert_eq!(
        BleEnvelope::command(&Packet::new(0x74, vec![0]).unwrap(), None)
            .encode(BleLength::Documented)
            .unwrap(),
        hex("feefaa5504000074007800")
    );
    for rule in [BleLength::Documented, BleLength::IdentifierIncluded] {
        for id in [None, Some(0), Some(1), Some(0x01020304), Some(u32::MAX)] {
            let env = BleEnvelope {
                packet_number: id,
                data: vec![0, 1, 2, 3, 0xff],
            };
            let wire = env.encode(rule).unwrap();
            for stop in 0..=wire.len() {
                let mut decoder = BleDecoder::new(rule);
                let mut out = decoder.feed(&wire[..stop]).unwrap();
                out.extend(decoder.feed(&wire[stop..]).unwrap());
                assert_eq!(out, vec![env.clone()]);
                decoder.finish().unwrap();
            }
        }
    }
}

#[test]
fn ble_rejects_malformed_and_oversized_envelopes() {
    assert!(BleEnvelope {
        packet_number: Some(1),
        data: vec![0; 65535]
    }
    .encode(BleLength::Documented)
    .is_err());
    let good = BleEnvelope {
        packet_number: Some(1),
        data: vec![1],
    }
    .encode(BleLength::IdentifierIncluded)
    .unwrap();
    for index in [0, 4, 6, 7, good.len() - 1] {
        let mut bad = good.clone();
        bad[index] ^= 0x80;
        let mut d = BleDecoder::new(BleLength::IdentifierIncluded);
        assert!(d.feed(&bad).is_err() || d.finish().is_err());
    }
}

#[test]
fn deterministic_adversarial_byte_streams_never_panic_or_grow_unbounded() {
    let mut seed = 19u32;
    for _ in 0..2000 {
        let mut bytes = Vec::new();
        for _ in 0..73 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            bytes.push((seed >> 16) as u8);
        }
        for dialect in [ClassicDialect::Escaped, ClassicDialect::PixooMax] {
            let _ = Decoder::with_limit(dialect, 1024).feed(&bytes);
        }
        let _ = BleDecoder::new(BleLength::Documented).feed(&bytes);
    }
}
