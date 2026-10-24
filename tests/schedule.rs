use divoom_bluetooth::{
    commands::{schedule::*, ClockTime, Percent, Rgb, Weekdays},
    protocol::Reply,
};
fn pct(v: u8) -> Percent {
    Percent::new(v).unwrap()
}
fn time(h: u8, m: u8) -> ClockTime {
    ClockTime::new(h, m).unwrap()
}
fn fixture(command: ScheduleCommand, opcode: u8, payload: &[u8]) {
    let p = command.packet().unwrap();
    assert_eq!(p.opcode(), opcode);
    assert_eq!(p.payload(), payload);
}
fn reply(opcode: u8, data: Vec<u8>) -> Reply {
    Reply {
        opcode,
        status: 0x55,
        data,
    }
}
fn alarm() -> Alarm {
    Alarm {
        index: 9,
        enabled: true,
        time: time(23, 59),
        weekdays: Weekdays::new(0x41).unwrap(),
        mode: AlarmMode::Music,
        trigger: AlarmTrigger::Music,
        fm_frequency: [0x34, 0x12],
        volume: pct(100),
    }
}
fn memorial() -> Memorial {
    Memorial {
        index: 2,
        enabled: true,
        month: 12,
        day: 31,
        time: time(23, 59),
        has_animation: true,
        title: [b'A'; 32],
    }
}
fn scene() -> Scene {
    Scene {
        minutes: 255,
        mode: 7,
        enabled: true,
        fm_frequency: [0x34, 0x12],
        volume: 255,
        color: Rgb {
            r: 1,
            g: 128,
            b: 255,
        },
        light: 255,
    }
}

#[test]
fn official_schedule_requests_246_259_266_272_277() {
    fixture(ScheduleCommand::GetAlarms, 0x42, &[]);
    fixture(
        ScheduleCommand::Alarm(alarm()),
        0x43,
        &[9, 1, 23, 59, 0x41, 0, 1, 0x34, 0x12, 100],
    );
    for (mode, byte) in [
        (AlarmMode::Music, 0),
        (AlarmMode::Mode1, 1),
        (AlarmMode::Mode2, 2),
        (AlarmMode::Mode3, 3),
        (AlarmMode::Mode4, 4),
    ] {
        let mut a = alarm();
        a.mode = mode;
        a.trigger = AlarmTrigger::Animation;
        a.enabled = false;
        fixture(
            ScheduleCommand::Alarm(a),
            0x43,
            &[9, 0, 23, 59, 0x41, byte, 4, 0x34, 0x12, 100],
        );
    }
    fixture(
        ScheduleCommand::Memorial(memorial()),
        0x54,
        b"\x02\x01\x0c\x1f\x17\x3b\x01AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let mut m = memorial();
    m.index = 0;
    m.enabled = false;
    m.has_animation = false;
    m.month = 0;
    m.day = 0;
    fixture(
        ScheduleCommand::Memorial(m),
        0x54,
        b"\x00\x00\x00\x00\x17\x3b\x00AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    fixture(ScheduleCommand::GetMemorials, 0x53, &[]);
    for (enabled, flag) in [(false, 0), (true, 1)] {
        fixture(
            ScheduleCommand::AlarmListen {
                enabled,
                mode: 3,
                volume: pct(100),
            },
            0xa5,
            &[flag, 3, 100],
        );
        fixture(
            ScheduleCommand::SleepListen {
                enabled,
                mode: 9,
                volume: 255,
            },
            0xa3,
            &[flag, 9, 255],
        );
    }
    fixture(ScheduleCommand::AlarmVolume(pct(100)), 0xa6, &[100]);
    fixture(ScheduleCommand::AlarmVolume(pct(0)), 0xa6, &[0]);
    for (control, byte) in [
        (VoiceAlarmControl::Record, 0),
        (VoiceAlarmControl::Play, 1),
        (VoiceAlarmControl::Stop, 2),
    ] {
        fixture(
            ScheduleCommand::VoiceAlarm { control, index: 9 },
            0x82,
            &[byte, 9],
        );
    }
    fixture(
        ScheduleCommand::TimeManagement(TimeManagementRecord {
            total_records: 10,
            index: 9,
            start: time(0, 1),
            end: time(23, 59),
            sports_minutes: 255,
            voice: true,
            display_mode: 4,
            cycle_mode: 5,
            picture: vec![0xa1, 0xb2, 0xc3],
        }),
        0x56,
        &[10, 9, 0, 1, 23, 59, 255, 1, 4, 5, 3, 0, 0xa1, 0xb2, 0xc3],
    );
    fixture(
        ScheduleCommand::TimeManagement(TimeManagementRecord {
            total_records: 1,
            index: 0,
            start: time(0, 0),
            end: time(0, 0),
            sports_minutes: 0,
            voice: false,
            display_mode: 0,
            cycle_mode: 0,
            picture: vec![],
        }),
        0x56,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
    fixture(
        ScheduleCommand::TimeManagementControl(TimeManagementControl::Get),
        0x57,
        &[0],
    );
    fixture(
        ScheduleCommand::TimeManagementControl(TimeManagementControl::Disable),
        0x57,
        &[1],
    );
    fixture(ScheduleCommand::GetSleepScene, 0xa2, &[]);
    fixture(ScheduleCommand::SceneVolume(pct(100)), 0xa4, &[100]);
    fixture(ScheduleCommand::SceneVolume(pct(0)), 0xa4, &[0]);
    fixture(
        ScheduleCommand::SleepColor(Rgb {
            r: 0,
            g: 128,
            b: 255,
        }),
        0xad,
        &[0, 128, 255],
    );
    fixture(ScheduleCommand::SleepLight(0), 0xae, &[0]);
    fixture(ScheduleCommand::SleepLight(255), 0xae, &[255]);
    fixture(
        ScheduleCommand::SleepAutoOff(scene()),
        0x40,
        &[255, 7, 1, 0x34, 0x12, 255, 1, 128, 255, 255],
    );
    fixture(
        ScheduleCommand::SleepScene(scene()),
        0x41,
        &[255, 7, 1, 0x34, 0x12, 255, 1, 128, 255, 255],
    );
}

#[test]
fn pages248_251_animation_transfers_have_fixed_chunks_and_last_actual_size() {
    for (packets, opcode) in [
        (alarm_animation_packets(9, &vec![0x6a; 201]).unwrap(), 0x51),
        (
            memorial_animation_packets(2, &vec![0x6a; 201]).unwrap(),
            0x55,
        ),
    ] {
        let index = if opcode == 0x51 { 9 } else { 2 };
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].opcode(), opcode);
        assert_eq!(&packets[0].payload()[..4], &[index, 201, 0, 0]);
        assert_eq!(&packets[0].payload()[4..], &[0x6a; 200]);
        assert_eq!(packets[1].payload(), &[index, 201, 0, 1, 0x6a]);
    }
    assert!(alarm_animation_packets(10, &[1]).is_err());
    assert!(memorial_animation_packets(3, &[1]).is_err());
    assert!(alarm_animation_packets(0, &[]).is_err());
}

#[test]
fn pages246_250_259_266_schedule_replies_decode_all_fields() {
    let mut alarms = [9, 1, 23, 59, 0x41, 0, 1, 0x34, 0x12, 100].repeat(10);
    alarms.push(0xee);
    let decoded = decode_alarms(&reply(0x42, alarms), AlarmReplyLayout::Page247).unwrap();
    assert_eq!(decoded.value, vec![alarm(); 10]);
    assert_eq!(decoded.extension, [0xee]);
    let mut memorials = b"\x02\x01\x0c\x1f\x17\x3b\x01AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".repeat(10);
    memorials.push(0xee);
    let decoded = ScheduleReply::decode(&reply(0x53, memorials)).unwrap();
    assert_eq!(
        decoded.value,
        ScheduleReply::Memorials(vec![memorial(); 10])
    );
    assert_eq!(decoded.extension, [0xee]);
    for (flag, value) in [(0, false), (1, true)] {
        let d = ScheduleReply::decode(&reply(0x57, vec![flag, 0xee])).unwrap();
        assert_eq!(d.value, ScheduleReply::TimeManagementConfigured(value));
        assert_eq!(d.extension, [0xee]);
    }
    let d = ScheduleReply::decode(&reply(
        0xa2,
        vec![255, 7, 1, 0x34, 0x12, 255, 1, 128, 255, 255, 0xee],
    ))
    .unwrap();
    assert_eq!(d.value, ScheduleReply::SleepScene(scene()));
    assert_eq!(d.extension, [0xee]);
    let d = ScheduleReply::decode(&reply(0x82, vec![0xee])).unwrap();
    assert_eq!(d.value, ScheduleReply::VoiceAlarmAcknowledged);
    assert_eq!(d.extension, [0xee]);
    let mut many = b"\x02\x01\x0c\x1f\x17\x3b\x01AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".repeat(10);
    for i in 0..10 {
        many[i * 39] = i as u8;
    }
    let d = ScheduleReply::decode(&reply(0x53, many)).unwrap();
    match d.value {
        ScheduleReply::Memorials(v) => assert_eq!(v[9].index, 9),
        _ => panic!(),
    };
}

#[test]
fn schedule_bounds_and_invalid_replies() {
    let mut a = alarm();
    a.index = 10;
    assert!(ScheduleCommand::Alarm(a).packet().is_err());
    let mut m = memorial();
    m.index = 3;
    assert!(ScheduleCommand::Memorial(m).packet().is_err());
    let mut m = memorial();
    m.month = 4;
    m.day = 31;
    assert!(ScheduleCommand::Memorial(m).packet().is_err());
    assert!(ScheduleCommand::VoiceAlarm {
        control: VoiceAlarmControl::Record,
        index: 10
    }
    .packet()
    .is_err());
    for (total_records, index) in [(0, 0), (11, 0), (1, 1)] {
        assert!(ScheduleCommand::TimeManagement(TimeManagementRecord {
            total_records,
            index,
            start: time(0, 0),
            end: time(0, 0),
            sports_minutes: 0,
            voice: false,
            display_mode: 0,
            cycle_mode: 0,
            picture: vec![]
        })
        .packet()
        .is_err());
    }
    assert!(ScheduleCommand::TimeManagement(TimeManagementRecord {
        total_records: 1,
        index: 0,
        start: time(0, 0),
        end: time(0, 0),
        sports_minutes: 0,
        voice: false,
        display_mode: 0,
        cycle_mode: 0,
        picture: vec![0; 65536]
    })
    .packet()
    .is_err());
    for (op, n) in [(0x53, 390), (0xa2, 10), (0x57, 1)] {
        assert!(ScheduleReply::decode(&reply(op, vec![0; n - 1])).is_err());
    }
    assert!(decode_alarms(&reply(0x42, vec![0; 99]), AlarmReplyLayout::Page247).is_err());
    assert!(decode_alarms(&reply(0x43, vec![0; 100]), AlarmReplyLayout::Page247).is_err());
    let mut raw = [9, 1, 23, 59, 0x41, 0, 1, 0x34, 0x12, 100].repeat(10);
    for (index, bad) in [
        (0, 10),
        (1, 2),
        (2, 24),
        (3, 60),
        (4, 128),
        (5, 5),
        (6, 2),
        (9, 101),
    ] {
        let old = raw[index];
        raw[index] = bad;
        assert!(decode_alarms(&reply(0x42, raw.clone()), AlarmReplyLayout::Page247).is_err());
        raw[index] = old;
    }
    assert!(ScheduleReply::decode(&reply(0x57, vec![2])).is_err());
    assert!(ScheduleReply::decode(&reply(0xfe, vec![])).is_err());
    let mut failed = reply(0x82, vec![]);
    failed.status = 0;
    assert!(ScheduleReply::decode(&failed).is_err());
    assert_eq!(ScheduleCommand::GetAlarms.response_opcode(), Some(0x42));
    assert_eq!(ScheduleCommand::SleepLight(2).response_opcode(), None);
}
