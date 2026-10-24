use divoom_bluetooth::{commands::tools::*, protocol::Reply};
fn fixture(command: ToolCommand, opcode: u8, payload: &[u8]) {
    let p = command.packet().unwrap();
    assert_eq!(p.opcode(), opcode);
    assert_eq!(p.payload(), payload);
}
fn reply(data: &[u8]) -> Reply {
    Reply {
        opcode: 0x71,
        status: 0x55,
        data: data.to_vec(),
    }
}

#[test]
fn pages264_265_tool_controls() {
    for (kind, byte) in [
        (ToolKind::Stopwatch, 0),
        (ToolKind::Score, 1),
        (ToolKind::Noise, 2),
        (ToolKind::Countdown, 3),
        (ToolKind::None, 255),
    ] {
        fixture(ToolCommand::Get(kind), 0x71, &[byte]);
    }
    for (control, byte) in [
        (StopwatchControl::Pause, 0),
        (StopwatchControl::Start, 1),
        (StopwatchControl::Reset, 2),
    ] {
        fixture(ToolCommand::Stopwatch(control), 0x72, &[0, byte]);
    }
    fixture(
        ToolCommand::Score {
            enabled: true,
            red: 0x1234,
            blue: 0x5678,
        },
        0x72,
        &[1, 1, 0x34, 0x12, 0x78, 0x56],
    );
    fixture(
        ToolCommand::Score {
            enabled: false,
            red: 0,
            blue: 65535,
        },
        0x72,
        &[1, 0, 0, 0, 255, 255],
    );
    fixture(ToolCommand::Noise(NoiseControl::Start), 0x72, &[2, 1]);
    fixture(ToolCommand::Noise(NoiseControl::Stop), 0x72, &[2, 2]);
    for (control, byte) in [
        (CountdownControl::Start, 0),
        (CountdownControl::Cancel, 1),
        (CountdownControl::TogglePause, 2),
    ] {
        fixture(
            ToolCommand::Countdown {
                control,
                minutes: 255,
                seconds: 59,
            },
            0x72,
            &[3, byte, 255, 59],
        );
    }
    fixture(
        ToolCommand::Countdown {
            control: CountdownControl::Start,
            minutes: 0,
            seconds: 0,
        },
        0x72,
        &[3, 0, 0, 0],
    );
    assert!(ToolCommand::Countdown {
        control: CountdownControl::Start,
        minutes: 0,
        seconds: 60
    }
    .packet()
    .is_err());
}

#[test]
fn page264_replies_are_decoded_using_requested_tool_kind() {
    for (byte, status) in [
        (0, StopwatchStatus::Paused),
        (1, StopwatchStatus::Started),
        (2, StopwatchStatus::Reset),
        (3, StopwatchStatus::Entering),
    ] {
        let d = ToolReply::decode(&reply(&[byte, 0xee]), ToolKind::Stopwatch).unwrap();
        assert_eq!(d.value, ToolReply::Stopwatch(status));
        assert_eq!(d.extension, [0xee]);
    }
    for (flag, enabled) in [(0, false), (1, true)] {
        let d = ToolReply::decode(
            &reply(&[flag, 0x34, 0x12, 0x78, 0x56, 0xee]),
            ToolKind::Score,
        )
        .unwrap();
        assert_eq!(
            d.value,
            ToolReply::Score {
                enabled,
                red: 0x1234,
                blue: 0x5678
            }
        );
        assert_eq!(d.extension, [0xee]);
    }
    for (byte, value) in [(1, NoiseControl::Start), (2, NoiseControl::Stop)] {
        let d = ToolReply::decode(&reply(&[byte, 0xee]), ToolKind::Noise).unwrap();
        assert_eq!(d.value, ToolReply::Noise(value));
        assert_eq!(d.extension, [0xee]);
    }
    for (byte, status) in [
        (0, CountdownStatus::Started),
        (1, CountdownStatus::Cancelled),
    ] {
        let d = ToolReply::decode(&reply(&[byte, 255, 59, 0xee]), ToolKind::Countdown).unwrap();
        assert_eq!(
            d.value,
            ToolReply::Countdown {
                status,
                minutes: 255,
                seconds: 59
            }
        );
        assert_eq!(d.extension, [0xee]);
    }
    let d = ToolReply::decode(&reply(&[0xee]), ToolKind::None).unwrap();
    assert_eq!(d.value, ToolReply::None);
    assert_eq!(d.extension, [0xee]);
    for (kind, data) in [
        (ToolKind::Stopwatch, vec![]),
        (ToolKind::Stopwatch, vec![4]),
        (ToolKind::Score, vec![1, 0, 0, 0]),
        (ToolKind::Score, vec![2, 0, 0, 0, 0]),
        (ToolKind::Noise, vec![0]),
        (ToolKind::Countdown, vec![2, 0, 0]),
        (ToolKind::Countdown, vec![0, 0, 60]),
    ] {
        assert!(ToolReply::decode(&reply(&data), kind).is_err());
    }
    let mut wrong = reply(&[0]);
    wrong.opcode = 0x72;
    assert!(ToolReply::decode(&wrong, ToolKind::Stopwatch).is_err());
    wrong.opcode = 0x71;
    wrong.status = 0;
    assert!(ToolReply::decode(&wrong, ToolKind::Stopwatch).is_err());
}

#[test]
fn pages278_281_game_controls_and_progress_notification() {
    fixture(ToolCommand::GameShake, 0x88, &[]);
    fixture(ToolCommand::GameEnabled(false), 0xa0, &[0]);
    fixture(ToolCommand::GameEnabled(true), 0xa0, &[1]);
    for (key, byte) in [
        (GameKey::Left, 1),
        (GameKey::Right, 2),
        (GameKey::Up, 3),
        (GameKey::Down, 4),
        (GameKey::Ok, 5),
    ] {
        fixture(ToolCommand::KeyDown(key), 0x17, &[byte]);
        fixture(ToolCommand::KeyUp(key), 0x21, &[byte]);
    }
    let d = GameProgress::decode_event(0x17, &[7, 0x34, 0x12, 0x78, 0x56, 0xee]).unwrap();
    assert_eq!(
        d.value,
        GameProgress {
            game_type: 7,
            seconds: 0x1234,
            score: 0x5678
        }
    );
    assert_eq!(d.extension, [0xee]);
    assert!(GameProgress::decode_event(0x17, &[0; 4]).is_err());
    assert!(GameProgress::decode_event(0x21, &[0; 5]).is_err());
    assert_eq!(
        ToolCommand::Get(ToolKind::Stopwatch).response_opcode(),
        Some(0x71)
    );
    assert_eq!(ToolCommand::KeyUp(GameKey::Ok).response_opcode(), None);
}
