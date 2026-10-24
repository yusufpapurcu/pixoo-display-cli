use divoom_bluetooth::{
    commands::{system::*, ClockTime, Percent},
    protocol::Reply,
};

fn fixture(command: SystemCommand, opcode: u8, payload: &[u8]) {
    let p = command.packet().unwrap();
    assert_eq!(p.opcode(), opcode);
    assert_eq!(p.payload(), payload);
}
fn reply(opcode: u8, data: &[u8]) -> Reply {
    Reply {
        opcode,
        status: 0x55,
        data: data.to_vec(),
    }
}
fn percent(v: u8) -> Percent {
    Percent::new(v).unwrap()
}

#[test]
fn official_system_requests_147_178_214_314_315() {
    fixture(SystemCommand::Brightness(percent(100)), 0x74, &[100]);
    fixture(SystemCommand::Brightness(percent(0)), 0x74, &[0]);
    let modes = [
        WorkMode::Bluetooth,
        WorkMode::Fm,
        WorkMode::LineIn,
        WorkMode::Sd,
        WorkMode::UsbHost,
        WorkMode::Record,
        WorkMode::RecordPlay,
        WorkMode::Uac,
        WorkMode::Phone,
        WorkMode::DivoomShow,
        WorkMode::AlarmSet,
        WorkMode::Game,
    ];
    for (mode, byte) in modes
        .into_iter()
        .zip([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11])
    {
        fixture(SystemCommand::WorkMode(mode), 5, &[byte]);
    }
    fixture(SystemCommand::GetWorkMode, 0x13, &[]);
    fixture(SystemCommand::GetSdStatus, 4, &[0x15]);
    fixture(SystemCommand::GetDeviceTemperature, 0x59, &[]);
    fixture(
        SystemCommand::NetworkWeather(WeatherReport {
            year: 2024,
            month: 2,
            day: 29,
            time: ClockTime::new(23, 59).unwrap(),
            weather: vec![
                Weather {
                    temperature: -12,
                    kind: 7,
                },
                Weather {
                    temperature: 34,
                    kind: 1,
                },
            ],
        }),
        0x5d,
        &[0xe8, 7, 2, 29, 23, 59, 2, 244, 7, 34, 1],
    );
    fixture(
        SystemCommand::WeatherDisplay(WeatherDisplay {
            modes: [true, false, true, false, true],
            minute_of_day: 1439,
        }),
        0x5e,
        &[1, 0, 1, 0, 1, 159, 5],
    );
    fixture(
        SystemCommand::CurrentWeather(Weather {
            temperature: -128,
            kind: 255,
        }),
        0x5f,
        &[128, 255],
    );
    fixture(SystemCommand::GetWeatherDisplay, 0x73, &[]);
    fixture(
        SystemCommand::DeviceName("猫".into()),
        0x75,
        &[3, 0xe7, 0x8c, 0xab],
    );
    fixture(SystemCommand::DeviceName("".into()), 0x75, &[0]);
    fixture(
        SystemCommand::DeviceName("0123456789abcdef".into()),
        0x75,
        b"\x100123456789abcdef",
    );
    fixture(SystemCommand::GetDeviceName, 0x76, &[]);
    for (value, byte) in [(false, 0), (true, 1)] {
        fixture(SystemCommand::LowPower(value), 0xb2, &[byte]);
        fixture(
            SystemCommand::SongNameDisplay(Setting::Set(value)),
            0x83,
            &[byte],
        );
        fixture(SystemCommand::Playing(value), 10, &[byte]);
        fixture(SystemCommand::NextTrack(value), 0x12, &[byte]);
        fixture(SystemCommand::SoundControl(value), 0xa7, &[byte]);
    }
    fixture(SystemCommand::GetLowPower, 0xb3, &[]);
    fixture(
        SystemCommand::TemperatureUnit(Setting::Set(TemperatureUnit::Celsius)),
        0x2b,
        &[0],
    );
    fixture(
        SystemCommand::TemperatureUnit(Setting::Set(TemperatureUnit::Fahrenheit)),
        0x2b,
        &[1],
    );
    fixture(SystemCommand::TemperatureUnit(Setting::Get), 0x2b, &[255]);
    fixture(
        SystemCommand::HourFormat(Setting::Set(HourFormat::Twelve)),
        0x2c,
        &[0],
    );
    fixture(
        SystemCommand::HourFormat(Setting::Set(HourFormat::TwentyFour)),
        0x2c,
        &[1],
    );
    fixture(SystemCommand::HourFormat(Setting::Get), 0x2c, &[255]);
    fixture(SystemCommand::SongNameDisplay(Setting::Get), 0x83, &[255]);
    fixture(SystemCommand::Password(PasswordControl::Clear), 0x27, &[0]);
    fixture(SystemCommand::Password(PasswordControl::Get), 0x27, &[2]);
    fixture(
        SystemCommand::Password(PasswordControl::Set {
            digits: [1, 2, 0, 9],
            encoding: PasswordEncoding::NumericDigits,
        }),
        0x27,
        &[1, 1, 2, 0, 9],
    );
    fixture(
        SystemCommand::Password(PasswordControl::Set {
            digits: [1, 2, 0, 9],
            encoding: PasswordEncoding::AsciiDigits,
        }),
        0x27,
        &[1, 49, 50, 48, 57],
    );
    fixture(SystemCommand::PowerOnVolume(Setting::Get), 0xbb, &[0, 0]);
    fixture(
        SystemCommand::PowerOnVolume(Setting::Set(percent(100))),
        0xbb,
        &[1, 100],
    );
    fixture(SystemCommand::PowerOnChannel(Setting::Get), 0x8a, &[0, 0]);
    for (channel, byte) in [
        PowerOnChannel::TimeFace,
        PowerOnChannel::Nightlight,
        PowerOnChannel::Cloud,
        PowerOnChannel::Vj,
        PowerOnChannel::MusicEq,
        PowerOnChannel::Custom,
    ]
    .into_iter()
    .zip([0, 1, 2, 3, 4, 5])
    {
        fixture(
            SystemCommand::PowerOnChannel(Setting::Set(channel)),
            0x8a,
            &[1, byte],
        );
    }
    fixture(SystemCommand::AutoPowerOff(720), 0xab, &[0xd0, 2]);
    fixture(SystemCommand::GetAutoPowerOff, 0xac, &[]);
    fixture(SystemCommand::GetSdTrackName, 6, &[]);
    fixture(
        SystemCommand::SdMusicList {
            start: 0x1234,
            end: 0x5678,
        },
        7,
        &[0x34, 0x12, 0x78, 0x56],
    );
    fixture(SystemCommand::Volume(Volume::new(15).unwrap()), 8, &[15]);
    fixture(SystemCommand::Volume(Volume::new(0).unwrap()), 8, &[0]);
    fixture(SystemCommand::GetVolume, 9, &[]);
    fixture(SystemCommand::GetPlaying, 11, &[]);
    fixture(SystemCommand::SdTrack(0x1234), 0x11, &[0x34, 0x12]);
    fixture(SystemCommand::GetSdTrackCount, 0x7d, &[]);
    fixture(SystemCommand::GetSdPlayback, 0xb4, &[]);
    fixture(
        SystemCommand::SdPlayback(SdPlayback {
            position_seconds: 0x1234,
            music_id: 0x5678,
            volume: Volume::new(15).unwrap(),
            playing: true,
            mode: PlayMode::Shuffle,
        }),
        0xb5,
        &[0x34, 0x12, 0x78, 0x56, 15, 1, 3],
    );
    fixture(SystemCommand::SdPosition(0x1234), 0xb8, &[0x34, 0x12]);
    for (mode, byte) in [
        (PlayMode::ListLoop, 1),
        (PlayMode::SingleLoop, 2),
        (PlayMode::Shuffle, 3),
    ] {
        fixture(SystemCommand::SdPlayMode(mode), 0xb9, &[byte]);
    }
    fixture(SystemCommand::RequestMusicList, 0x47, &[]);
    fixture(SystemCommand::GetSoundControl, 0xa8, &[]);
}

#[test]
fn official_system_replies_preserve_all_fields_and_extensions() {
    let cases = vec![
        (0x13, vec![11], SystemReply::WorkMode(WorkMode::Game)),
        (0x15, vec![1], SystemReply::SdInserted(true)),
        (
            0x59,
            vec![1, 244],
            SystemReply::DeviceTemperature {
                unit: TemperatureUnit::Fahrenheit,
                value: -12,
            },
        ),
        (
            0x73,
            vec![1, 0, 1, 0, 1, 159, 5],
            SystemReply::WeatherDisplay(WeatherDisplay {
                modes: [true, false, true, false, true],
                minute_of_day: 1439,
            }),
        ),
        (
            0x76,
            vec![3, 0xe7, 0x8c, 0xab],
            SystemReply::DeviceName("猫".into()),
        ),
        (0xb3, vec![0], SystemReply::LowPower(false)),
        (
            0x2b,
            vec![0],
            SystemReply::TemperatureUnit(TemperatureUnit::Celsius),
        ),
        (
            0x2c,
            vec![1],
            SystemReply::HourFormat(HourFormat::TwentyFour),
        ),
        (0x83, vec![1], SystemReply::SongNameDisplay(true)),
        (0x27, vec![0], SystemReply::Password(None)),
        (
            0x27,
            vec![1, 49, 50, 51, 52],
            SystemReply::Password(Some([49, 50, 51, 52])),
        ),
        (0xbb, vec![100], SystemReply::PowerOnVolume(percent(100))),
        (
            0x8a,
            vec![5],
            SystemReply::PowerOnChannel(PowerOnChannel::Custom),
        ),
        (0xac, vec![0xd0, 2], SystemReply::AutoPowerOff(720)),
        (
            6,
            vec![2, 0, 0x67, 0x9c],
            SystemReply::SdName(SdName {
                music_id: None,
                unicode_bytes: vec![0x67, 0x9c],
            }),
        ),
        (
            7,
            vec![0x34, 0x12, 2, 0, 0x67, 0x9c],
            SystemReply::SdName(SdName {
                music_id: Some(0x1234),
                unicode_bytes: vec![0x67, 0x9c],
            }),
        ),
        (9, vec![15], SystemReply::Volume(Volume::new(15).unwrap())),
        (11, vec![0], SystemReply::Playing(false)),
        (0x11, vec![], SystemReply::Acknowledged(0x11)),
        (0x14, vec![], SystemReply::SdListComplete),
        (0x7d, vec![0x34, 0x12], SystemReply::SdTrackCount(0x1234)),
        (
            0xb4,
            vec![0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a, 1, 15, 2],
            SystemReply::SdPlayback {
                playback: SdPlayback {
                    position_seconds: 0x1234,
                    music_id: 0x9abc,
                    volume: Volume::new(15).unwrap(),
                    playing: true,
                    mode: PlayMode::SingleLoop,
                },
                total_seconds: 0x5678,
            },
        ),
        (0x47, vec![], SystemReply::MusicListReady),
        (0xa8, vec![1], SystemReply::SoundControl(true)),
    ];
    for (op, mut data, value) in cases {
        let minimum = data.len();
        data.push(0xee);
        let decoded = SystemReply::decode(&reply(op, &data)).unwrap();
        assert_eq!(decoded.value, value);
        assert_eq!(decoded.extension, [0xee]);
        if minimum > 0 {
            assert!(
                SystemReply::decode(&reply(op, &data[..minimum - 1])).is_err(),
                "opcode {op}"
            );
        }
    }
    assert_eq!(
        SystemReply::decode_event(0x13, &[3, 77]).unwrap(),
        Decoded {
            value: SystemReply::WorkMode(WorkMode::Sd),
            extension: vec![77]
        }
    );
    assert_eq!(
        SystemReply::decode_event(0x14, &[77]).unwrap().extension,
        [77]
    );
}

#[test]
fn page181_boot_animation_200_byte_chunks_and_sequence_bounds() {
    let data = vec![0x6a; 201];
    let packets = boot_animation_packets(true, &data).unwrap();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].opcode(), 0x52);
    assert_eq!(&packets[0].payload()[..4], &[1, 201, 0, 0]);
    assert_eq!(&packets[0].payload()[4..], &[0x6a; 200]);
    assert_eq!(packets[1].payload(), &[1, 201, 0, 1, 0x6a]);
    assert_eq!(
        boot_animation_packets(false, &[0x10]).unwrap()[0].payload(),
        &[0, 1, 0, 0, 0x10]
    );
    let maximum = boot_animation_packets(true, &vec![7; 51200]).unwrap();
    assert_eq!(maximum.len(), 256);
    assert_eq!(&maximum[255].payload()[..4], &[1, 0, 200, 255]);
    assert!(boot_animation_packets(true, &[]).is_err());
    assert!(boot_animation_packets(true, &vec![0; 51201]).is_err());
}

#[test]
fn system_validation_and_reply_errors() {
    assert!(Percent::new(101).is_err());
    assert!(Volume::new(16).is_err());
    assert!(SystemCommand::DeviceName("abcdefghijklmnopq".into())
        .packet()
        .is_err());
    assert!(SystemCommand::Password(PasswordControl::Set {
        digits: [0, 1, 2, 10],
        encoding: PasswordEncoding::NumericDigits
    })
    .packet()
    .is_err());
    assert!(SystemCommand::SdMusicList { start: 2, end: 1 }
        .packet()
        .is_err());
    assert!(SystemCommand::WeatherDisplay(WeatherDisplay {
        modes: [true; 5],
        minute_of_day: 1440
    })
    .packet()
    .is_err());
    for (year, month, day) in [(2023, 2, 29), (2024, 0, 1), (2024, 4, 31), (2024, 1, 0)] {
        assert!(SystemCommand::NetworkWeather(WeatherReport {
            year,
            month,
            day,
            time: ClockTime::new(0, 0).unwrap(),
            weather: vec![]
        })
        .packet()
        .is_err());
    }
    assert!(SystemCommand::NetworkWeather(WeatherReport {
        year: 2024,
        month: 1,
        day: 1,
        time: ClockTime::new(0, 0).unwrap(),
        weather: vec![
            Weather {
                temperature: 0,
                kind: 0
            };
            256
        ]
    })
    .packet()
    .is_err());
    for (op, data) in [
        (0x13, vec![12]),
        (0x15, vec![2]),
        (0x59, vec![2, 0]),
        (0x73, vec![1, 1, 1, 1, 1, 0xa0, 5]),
        (0x76, vec![1, 255]),
        (0xb3, vec![2]),
        (0x2b, vec![2]),
        (0x2c, vec![2]),
        (0x83, vec![2]),
        (0x27, vec![2]),
        (0xbb, vec![101]),
        (0x8a, vec![6]),
        (9, vec![16]),
        (11, vec![2]),
        (0xa8, vec![2]),
    ] {
        assert!(SystemReply::decode(&reply(op, &data)).is_err());
    }
    let mut failed = reply(9, &[3]);
    failed.status = 0;
    assert!(SystemReply::decode(&failed).is_err());
    assert!(SystemReply::decode(&reply(0xfe, &[])).is_err());
    assert_eq!(SystemCommand::GetVolume.response_opcode(), Some(9));
    assert_eq!(SystemCommand::Playing(true).response_opcode(), None);
}
