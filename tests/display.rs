use divoom_bluetooth::{
    commands::{display::*, Percent, Rgb},
    protocol::Reply,
};

fn rgb() -> Rgb {
    Rgb {
        r: 0x12,
        g: 0x34,
        b: 0x56,
    }
}
fn image() -> EncodedImageBlock {
    EncodedImageBlock::new(vec![0xaa, 0xbb]).unwrap()
}
fn chunk() -> AnimationChunk {
    AnimationChunk::new(2, 0, vec![0xaa, 0xbb]).unwrap()
}
fn file_chunk() -> FileChunk {
    FileChunk::new(258, 1, vec![0xaa, 0xbb]).unwrap()
}
fn offsets() -> EncodedPointOffsets {
    EncodedPointOffsets::from_u8(vec![7, 19]).unwrap()
}
fn check(command: DisplayCommand, opcode: u8, payload: &[u8]) {
    let packet = command.packet().unwrap();
    assert_eq!(packet.opcode(), opcode);
    assert_eq!(packet.payload(), payload);
}
fn reply(opcode: u8, data: &[u8]) -> Reply {
    Reply {
        opcode,
        status: 0x55,
        data: data.to_vec(),
    }
}

#[test]
fn page287_every_light_mode() {
    check(
        DisplayCommand::SetLightMode(LightMode::Environment {
            time_format: TimeFormat::TwentyFourHour,
            display_mode: 7,
            checkboxes: [1, 0, 2, 3],
            color: rgb(),
        }),
        0x45,
        &[0, 1, 7, 1, 0, 2, 3, 0x12, 0x34, 0x56],
    );
    check(
        DisplayCommand::SetLightMode(LightMode::Environment {
            time_format: TimeFormat::TwelveHour,
            display_mode: 0,
            checkboxes: [0; 4],
            color: rgb(),
        }),
        0x45,
        &[0, 0, 0, 0, 0, 0, 0, 0x12, 0x34, 0x56],
    );
    check(
        DisplayCommand::SetLightMode(LightMode::Light {
            color: rgb(),
            brightness: Percent::new(75).unwrap(),
            effect: 2,
            on: true,
        }),
        0x45,
        &[1, 0x12, 0x34, 0x56, 75, 2, 1],
    );
    check(DisplayCommand::SetLightMode(LightMode::Divoom), 0x45, &[2]);
    check(
        DisplayCommand::SetLightMode(LightMode::Special { selection: 9 }),
        0x45,
        &[3, 9],
    );
    check(
        DisplayCommand::SetLightMode(LightMode::Music { selection: 8 }),
        0x45,
        &[4, 8],
    );
    check(
        DisplayCommand::SetLightMode(LightMode::UserDefined),
        0x45,
        &[5],
    );
    check(
        DisplayCommand::SetLightMode(LightMode::Score {
            on: false,
            red: 200,
            blue: 13,
        }),
        0x45,
        &[6, 0, 200, 13],
    );
}

#[test]
fn page288_light_settings_and_extensions() {
    check(DisplayCommand::GetLightMode, 0x46, &[]);
    let decoded = LightSettings::decode(&reply(
        0x46,
        &[
            5, 1, 8, 0x12, 0x34, 0x56, 90, 2, 1, 4, 65, 1, 9, 10, 11, 3, 0, 1, 2, 3, 0xf0,
        ],
    ))
    .unwrap();
    assert_eq!(decoded.mode, CurrentLightMode::Custom);
    assert_eq!(decoded.temperature_mode, 1);
    assert_eq!(decoded.vj_selection, 8);
    assert_eq!(decoded.color, rgb());
    assert_eq!(decoded.brightness, 90);
    assert_eq!(
        (
            decoded.light_selection,
            decoded.light_switch,
            decoded.music_selection
        ),
        (2, 1, 4)
    );
    assert_eq!(
        (
            decoded.system_brightness,
            decoded.time_format,
            decoded.time_display_mode
        ),
        (65, 1, 3)
    );
    assert_eq!(decoded.time_color, Rgb { r: 9, g: 10, b: 11 });
    assert_eq!(decoded.time_checkboxes, [0, 1, 2, 3]);
    assert_eq!(decoded.extensions, [0xf0]);
    for (value, mode) in [
        (0, CurrentLightMode::Clock),
        (1, CurrentLightMode::NightLight),
        (2, CurrentLightMode::Hot),
        (3, CurrentLightMode::Vj),
        (4, CurrentLightMode::MusicEqOrFlag),
        (6, CurrentLightMode::Lyric),
        (200, CurrentLightMode::Unknown(200)),
    ] {
        let mut data = vec![0; 20];
        data[0] = value;
        assert_eq!(
            LightSettings::decode(&reply(0x46, &data)).unwrap().mode,
            mode
        );
    }
}

#[test]
fn pages289_290_291_encoded_payload_and_upload() {
    check(DisplayCommand::Picture(image()), 0x44, &[0xaa, 0xbb]);
    check(
        DisplayCommand::PhoneAnimation(chunk()),
        0x49,
        &[2, 0, 0, 0xaa, 0xbb],
    );
    let (decoded, tail) =
        AnimationChunk::decode_upload(&reply(0x49, &[2, 0, 0, 0xaa, 0xbb, 0x99])).unwrap();
    assert_eq!(decoded, chunk());
    assert_eq!(tail, [0x99]);
    check(
        DisplayCommand::GifSpeed {
            milliseconds: 0x1234,
        },
        0x16,
        &[0x34, 0x12],
    );
}

#[test]
fn page292_every_text_attribute() {
    let cases = [
        (
            TextAttribute::Speed {
                milliseconds: 0x1234,
                text_box: 2,
            },
            vec![1, 0x34, 0x12, 2],
        ),
        (TextAttribute::Effect { style: 5 }, vec![2, 5]),
        (
            TextAttribute::DisplayBox {
                x: 10,
                y: 20,
                width: 32,
                height: 16,
                text_box: 3,
            },
            vec![3, 10, 20, 32, 16, 3],
        ),
        (
            TextAttribute::Font {
                size: FontSize::Points16,
                text_box: 4,
            },
            vec![4, 16, 4],
        ),
        (
            TextAttribute::Font {
                size: FontSize::Points24,
                text_box: 4,
            },
            vec![4, 24, 4],
        ),
        (
            TextAttribute::Font {
                size: FontSize::Points32,
                text_box: 4,
            },
            vec![4, 32, 4],
        ),
        (
            TextAttribute::Color {
                color: rgb(),
                text_box: 5,
            },
            vec![5, 0x12, 0x34, 0x56, 5],
        ),
        (
            TextAttribute::Content {
                frame: EncodedWordFrame::new(vec![0xde, 0xad]).unwrap(),
                text_box: 6,
            },
            vec![6, 2, 0, 0xde, 0xad, 6],
        ),
        (
            TextAttribute::ImageEffect {
                style: 7,
                text_box: 8,
            },
            vec![7, 7, 8],
        ),
    ];
    for (attribute, bytes) in cases {
        check(DisplayCommand::Text(attribute), 0x87, &bytes);
    }
}

#[test]
fn page293_all_transfer_controls_and_replies() {
    check(
        DisplayCommand::GifTransfer(GifTransfer::Start { size: 0x00120304 }),
        0x8b,
        &[0, 4, 3, 0x12, 0],
    );
    check(
        DisplayCommand::GifTransfer(GifTransfer::Data(file_chunk())),
        0x8b,
        &[1, 2, 1, 0, 0, 1, 0, 0xaa, 0xbb],
    );
    check(DisplayCommand::GifTransfer(GifTransfer::End), 0x8b, &[2]);
    assert_eq!(
        TransferReply::decode(&reply(0x8b, &[0, 1, 0xee])).unwrap(),
        TransferReply::Ready {
            allowed: true,
            extensions: vec![0xee]
        }
    );
    assert_eq!(
        TransferReply::decode(&reply(0x8b, &[0, 0])).unwrap(),
        TransferReply::Ready {
            allowed: false,
            extensions: vec![]
        }
    );
    assert_eq!(
        TransferReply::decode(&reply(0x8b, &[1, 0x34, 0x12, 0xee])).unwrap(),
        TransferReply::Resend {
            sequence: 0x1234,
            extensions: vec![0xee]
        }
    );
}

#[test]
fn page294_all_save_controls_and_metadata() {
    for (kind, bytes) in [
        (SavedArtworkKind::NormalImage, vec![0]),
        (
            SavedArtworkKind::LedEditor {
                speed: 5,
                text: vec![0x41, 0x20ac],
            },
            vec![1, 5, 2, 0x41, 0, 0xac, 0x20],
        ),
        (SavedArtworkKind::SandPainting, vec![2]),
        (
            SavedArtworkKind::ScrollAnimation {
                mode: 4,
                speed_ms: 0x1234,
                length: 0x5678,
            },
            vec![3, 4, 0x34, 0x12, 0x78, 0x56],
        ),
    ] {
        let mut start = vec![0];
        start.extend(&bytes);
        let mut end = vec![2];
        end.extend(bytes);
        check(
            DisplayCommand::UserGif(UserGif::Start(kind.clone())),
            0xb1,
            &start,
        );
        check(DisplayCommand::UserGif(UserGif::End(kind)), 0xb1, &end);
    }
    check(
        DisplayCommand::UserGif(UserGif::Frame(image())),
        0xb1,
        &[1, 2, 0, 0xaa, 0xbb],
    );
    let sand = SandPaintingData {
        speed_ms: 0x1234,
        image: image(),
        strokes: vec![
            SandStroke::fill(rgb()),
            SandStroke::from_wire(2, rgb(), vec![9, 12]).unwrap(),
        ],
    };
    assert_eq!(
        sand.encode_u16_le().unwrap().bytes(),
        &[0x34, 0x12, 2, 0, 0xaa, 0xbb, 0, 0, 0x12, 0x34, 0x56, 2, 0, 0x12, 0x34, 0x56, 9, 12]
    );
    assert_eq!(
        decode_user_gif_ack(&reply(0xb1, &[0x0f, 1, 2])).unwrap(),
        [0x0f, 1, 2]
    );
}

#[test]
fn page295_query_delete_reply() {
    check(DisplayCommand::UserGifCount, 0xb6, &[255]);
    check(DisplayCommand::DeleteUserGif { item: 3 }, 0xb6, &[3]);
    assert_eq!(
        UserGifItemReply::decode(&reply(0xb6, &[3, 0xee])).unwrap(),
        UserGifItemReply {
            item: 3,
            extensions: vec![0xee]
        }
    );
}

#[test]
fn page296_all_indexed_controls_and_resend() {
    check(
        DisplayCommand::IndexedTransfer(IndexedTransfer::Start {
            size: 0x00120304,
            index: 7,
        }),
        0x8c,
        &[0, 4, 3, 0x12, 0, 7],
    );
    check(
        DisplayCommand::IndexedTransfer(IndexedTransfer::Data(file_chunk())),
        0x8c,
        &[1, 2, 1, 0, 0, 1, 0, 0xaa, 0xbb],
    );
    check(
        DisplayCommand::IndexedTransfer(IndexedTransfer::End),
        0x8c,
        &[2],
    );
    assert_eq!(
        TransferReply::decode(&reply(0x8c, &[1, 0x34, 0x12, 0xee])).unwrap(),
        TransferReply::Resend {
            sequence: 0x1234,
            extensions: vec![0xee]
        }
    );
}

#[test]
fn page297_all_big64_controls_and_explicit_id_layout() {
    let id = EncodedArtworkId::from_u32_le(0x12345678);
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::Start {
            size: 0x00120304,
            index: 2,
            file_id: id.clone(),
        }),
        0x8d,
        &[0, 4, 3, 0x12, 0, 2, 0x78, 0x56, 0x34, 0x12],
    );
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::Data(file_chunk())),
        0x8d,
        &[1, 2, 1, 0, 0, 1, 0, 0xaa, 0xbb],
    );
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::End),
        0x8d,
        &[2],
    );
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::Delete {
            file_id: id.clone(),
            index: 3,
        }),
        0x8d,
        &[3, 0x78, 0x56, 0x34, 0x12, 3],
    );
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::Play {
            file_id: id,
            index: 4,
        }),
        0x8d,
        &[4, 0x78, 0x56, 0x34, 0x12, 4],
    );
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::DeleteIndex { index: 5 }),
        0x8d,
        &[5, 5],
    );
    check(
        DisplayCommand::Big64Transfer(Big64Transfer::Play {
            file_id: EncodedArtworkId::from_wire(vec![0xaa, 0xbb]).unwrap(),
            index: 4,
        }),
        0x8d,
        &[4, 0xaa, 0xbb, 4],
    );
    assert_eq!(
        TransferReply::decode(&reply(0x8d, &[1, 0x34, 0x12])).unwrap(),
        TransferReply::Resend {
            sequence: 0x1234,
            extensions: vec![]
        }
    );
    assert_eq!(
        TransferReply::decode(&reply(0x8d, &[0, 0xee])).unwrap(),
        TransferReply::Unknown {
            control: 0,
            data: vec![0xee]
        }
    );
}

#[test]
fn page298_artwork_metadata_pagination() {
    check(DisplayCommand::GetArtworkInfo { index: 9 }, 0x8e, &[9]);
    assert_eq!(
        ArtworkInfoReply::decode(&reply(
            0x8e,
            &[1, 9, 0x34, 0x12, 2, 0, 2, 0, 0x78, 0x56, 0x34, 0x12, 1, 2, 3, 4, 0xee]
        ))
        .unwrap(),
        ArtworkInfoReply::Files {
            index: 9,
            total: 0x1234,
            offset: 2,
            ids: vec![ArtworkId([0x78, 0x56, 0x34, 0x12]), ArtworkId([1, 2, 3, 4])],
            extensions: vec![0xee]
        }
    );
    assert_eq!(
        ArtworkInfoReply::decode(&reply(0x8e, &[2, 9, 0xee])).unwrap(),
        ArtworkInfoReply::End {
            index: 9,
            extensions: vec![0xee]
        }
    );
    assert_eq!(
        ArtworkInfoReply::decode(&reply(0x8e, &[99, 1, 2])).unwrap(),
        ArtworkInfoReply::Unknown {
            control: 99,
            data: vec![1, 2]
        }
    );
    assert_eq!(ArtworkId::from_u32(0x12345678).value(), 0x12345678);
}

#[test]
fn pages299_300_rhythm_and_eq() {
    check(
        DisplayCommand::RhythmAnimation {
            position: 3,
            chunk: chunk(),
        },
        0xb7,
        &[3, 2, 0, 0, 0xaa, 0xbb],
    );
    check(
        DisplayCommand::EqAnimation {
            position: 4,
            chunk: chunk(),
        },
        0x1b,
        &[4, 2, 0, 0, 0xaa, 0xbb],
    );
}

#[test]
fn pages301_302_multi_and_big_pad() {
    check(
        DisplayCommand::DrawMulti {
            screen: 2,
            color: rgb(),
            offsets: offsets(),
        },
        0x3a,
        &[2, 0x12, 0x34, 0x56, 2, 7, 19],
    );
    check(
        DisplayCommand::DrawBig {
            canvas_width_screens: 4,
            screen: 2,
            color: rgb(),
            offsets: EncodedPointOffsets::from_u16_le(&[0x1234, 0x5678]).unwrap(),
        },
        0x3b,
        &[4, 2, 0x12, 0x34, 0x56, 2, 0x34, 0x12, 0x78, 0x56],
    );
    for opcode in [0x3a, 0x3b] {
        for (flag, state) in [
            (0, DrawingState::RequestWholeImage),
            (1, DrawingState::Display),
            (7, DrawingState::Unknown(7)),
        ] {
            assert_eq!(
                DrawingReply::decode(&reply(opcode, &[flag, 0xee])).unwrap(),
                DrawingReply {
                    state,
                    extensions: vec![0xee]
                }
            );
        }
    }
}

#[test]
fn pages303_304_single_pad_explicit_ambiguous_layout_and_exit() {
    check(
        DisplayCommand::DrawSingle {
            color: rgb(),
            offsets: offsets(),
            layout: SinglePadLayout::ConsecutiveByteOrdinals,
        },
        0x58,
        &[0x12, 0x34, 0x56, 2, 7, 19],
    );
    check(
        DisplayCommand::DrawSingle {
            color: rgb(),
            offsets: offsets(),
            layout: SinglePadLayout::IndexedWithReservedByte(0xee),
        },
        0x58,
        &[0x12, 0x34, 0x56, 0xee, 2, 7, 19],
    );
    assert_eq!(
        decode_single_drawing_ack(&reply(0x58, &[0xee])).unwrap(),
        [0xee]
    );
    check(DisplayCommand::ExitDrawing, 0x5a, &[]);
}

#[test]
fn pages305_306_explicit_unspecified_length_widths() {
    for (width, picture, animation) in [
        (
            UnspecifiedLengthWidth::U8,
            vec![2, 2, 0xaa, 0xbb],
            vec![2, 2, 0, 0xaa, 0xbb],
        ),
        (
            UnspecifiedLengthWidth::U16LittleEndian,
            vec![2, 2, 0, 0xaa, 0xbb],
            vec![2, 2, 0, 0, 0xaa, 0xbb],
        ),
        (
            UnspecifiedLengthWidth::U32LittleEndian,
            vec![2, 2, 0, 0, 0, 0xaa, 0xbb],
            vec![2, 2, 0, 0, 0, 0, 0xaa, 0xbb],
        ),
    ] {
        check(
            DisplayCommand::MultiPicture {
                screen: 2,
                image: image(),
                length_width: width,
            },
            0x5b,
            &picture,
        );
        check(
            DisplayCommand::MultiAnimation {
                screen: 2,
                chunk: chunk(),
                length_width: width,
            },
            0x5c,
            &animation,
        );
    }
}

#[test]
fn pages307_308_309_310_311_movie_and_drawing_control() {
    check(DisplayCommand::PlayMultiAnimation, 0x6b, &[]);
    check(
        DisplayCommand::MovieFrame {
            frame_id: 0x1234,
            image: image(),
        },
        0x6c,
        &[0x34, 0x12, 2, 0, 0xaa, 0xbb],
    );
    check(
        DisplayCommand::MultiMovieFrame {
            screen: 7,
            frame_id: 0x1234,
            image: image(),
        },
        0x6d,
        &[7, 0x34, 0x12, 2, 0, 0xaa, 0xbb],
    );
    check(DisplayCommand::Movie(MovieControl::Exit), 0x6e, &[0]);
    check(DisplayCommand::Movie(MovieControl::Start), 0x6e, &[1]);
    check(
        DisplayCommand::EnterOrClearMultiDrawing { color: rgb() },
        0x6f,
        &[0x12, 0x34, 0x56],
    );
}

#[test]
fn pages312_313_sand_and_scroll_subcontrols() {
    check(
        DisplayCommand::SandInitialize {
            screen: 7,
            image: image(),
        },
        0x34,
        &[0, 7, 2, 0, 0xaa, 0xbb],
    );
    check(DisplayCommand::SandReset, 0x34, &[1]);
    check(
        DisplayCommand::ScrollSettings {
            mode: 7,
            speed_ms: 0x1234,
        },
        0x35,
        &[0, 7, 0x34, 0x12],
    );
    check(
        DisplayCommand::ScrollImage(chunk()),
        0x35,
        &[1, 2, 0, 0, 0xaa, 0xbb],
    );
}

#[test]
fn checked_bounds_and_packet_fit() {
    assert!(EncodedImageBlock::new(vec![]).is_err());
    assert!(EncodedImageBlock::new(vec![0; 65536]).is_err());
    assert!(EncodedWordFrame::new(vec![]).is_err());
    assert!(EncodedWordFrame::new(vec![0; 65536]).is_err());
    assert!(EncodedArtworkId::from_wire(vec![]).is_err());
    assert!(AnimationChunk::new(0, 0, vec![]).is_err());
    assert!(AnimationChunk::new(201, 0, vec![0; 199]).is_err());
    assert!(AnimationChunk::new(201, 1, vec![0; 2]).is_err());
    assert!(AnimationChunk::new(200, 1, vec![]).is_err());
    assert!(AnimationChunk::new(51201, 0, vec![0; 200]).is_err());
    assert!(AnimationChunk::new(51200, 255, vec![0; 200]).is_ok());
    assert!(FileChunk::new(0, 0, vec![]).is_err());
    assert!(FileChunk::new(257, 0, vec![0; 255]).is_err());
    assert!(FileChunk::new(257, 1, vec![0; 2]).is_err());
    assert!(FileChunk::new(256, 1, vec![]).is_err());
    assert!(FileChunk::new(16777217, 0, vec![0; 256]).is_err());
    assert!(FileChunk::new(16777216, 65535, vec![0; 256]).is_ok());
    assert!(EncodedPointOffsets::from_wire(0, vec![1]).is_err());
    assert!(EncodedPointOffsets::from_wire(1, vec![]).is_err());
    assert!(EncodedPointOffsets::from_u8(vec![0; 256]).is_err());
    assert!(EncodedPointOffsets::from_u16_le(&[0; 256]).is_err());
    assert!(SandStroke::from_wire(0, rgb(), vec![1]).is_err());
    assert!(SandStroke::from_wire(1, rgb(), vec![]).is_err());
    for command in [
        DisplayCommand::DeleteUserGif { item: 0 },
        DisplayCommand::DeleteUserGif { item: 255 },
        DisplayCommand::GifTransfer(GifTransfer::Start { size: 0 }),
        DisplayCommand::IndexedTransfer(IndexedTransfer::Start {
            size: 16777217,
            index: 0,
        }),
        DisplayCommand::Big64Transfer(Big64Transfer::Start {
            size: 0,
            index: 0,
            file_id: EncodedArtworkId::from_u32_le(1),
        }),
        DisplayCommand::RhythmAnimation {
            position: 0,
            chunk: chunk(),
        },
        DisplayCommand::EqAnimation {
            position: 0,
            chunk: chunk(),
        },
        DisplayCommand::DrawBig {
            canvas_width_screens: 0,
            screen: 0,
            color: rgb(),
            offsets: offsets(),
        },
        DisplayCommand::Text(TextAttribute::DisplayBox {
            x: 0,
            y: 0,
            width: 0,
            height: 1,
            text_box: 0,
        }),
        DisplayCommand::Text(TextAttribute::DisplayBox {
            x: 255,
            y: 0,
            width: 2,
            height: 1,
            text_box: 0,
        }),
        DisplayCommand::UserGif(UserGif::Start(SavedArtworkKind::LedEditor {
            speed: 1,
            text: vec![0; 256],
        })),
        DisplayCommand::MultiPicture {
            screen: 0,
            image: EncodedImageBlock::new(vec![0; 256]).unwrap(),
            length_width: UnspecifiedLengthWidth::U8,
        },
        DisplayCommand::MultiAnimation {
            screen: 0,
            chunk: AnimationChunk::new(256, 0, vec![0; 200]).unwrap(),
            length_width: UnspecifiedLengthWidth::U8,
        },
        DisplayCommand::Picture(EncodedImageBlock::new(vec![0; 65535]).unwrap()),
        DisplayCommand::MovieFrame {
            frame_id: 0,
            image: EncodedImageBlock::new(vec![0; 65530]).unwrap(),
        },
    ] {
        assert!(
            command.packet().is_err(),
            "unexpected valid command {command:?}"
        );
    }
    assert!(Percent::new(101).is_err());
}

#[test]
fn response_decoders_reject_wrong_opcode_status_and_truncation() {
    assert!(LightSettings::decode(&reply(0x45, &[0; 20])).is_err());
    assert!(LightSettings::decode(&reply(0x46, &[0; 19])).is_err());
    let rejected = Reply {
        opcode: 0x46,
        status: 0,
        data: vec![0; 20],
    };
    assert!(LightSettings::decode(&rejected).is_err());
    for bytes in [&[][..], &[0][..], &[0, 2][..], &[1, 1][..]] {
        assert!(TransferReply::decode(&reply(0x8b, bytes)).is_err());
    }
    assert!(TransferReply::decode(&reply(0xb1, &[0, 1])).is_err());
    for bytes in [
        &[][..],
        &[1, 0][..],
        &[1, 0, 1, 0, 0, 0, 1, 0][..],
        &[1, 0, 0, 0, 0, 0, 1, 0, 1, 2, 3, 4][..],
        &[2][..],
    ] {
        assert!(ArtworkInfoReply::decode(&reply(0x8e, bytes)).is_err());
    }
    assert!(UserGifItemReply::decode(&reply(0xb6, &[])).is_err());
    assert!(DrawingReply::decode(&reply(0x3a, &[])).is_err());
    assert!(DrawingReply::decode(&reply(0x58, &[1])).is_err());
    assert!(AnimationChunk::decode_upload(&reply(0x49, &[2, 0, 0, 0xaa])).is_err());
    assert!(AnimationChunk::decode_upload(&reply(0x49, &[0, 0, 0])).is_err());
    assert!(decode_user_gif_ack(&reply(0x8c, &[])).is_err());
    assert!(decode_single_drawing_ack(&Reply {
        opcode: 0x58,
        status: 0,
        data: vec![]
    })
    .is_err());
}

#[test]
fn structural_native_text_does_not_claim_wire_encoding() {
    let mut info = WordInfo {
        x: 0,
        y: 0,
        size: FontSize::Points16,
        width: 32,
        height: 16,
        font_effect: 0,
        picture_effect: 0,
        speed_ms: 100,
        color: rgb(),
        unicode: vec![0x41],
        fonts: vec![WordFont {
            unicode16: 0x41,
            font_info: [0; 32],
        }],
    };
    info.validate().unwrap();
    info.fonts.push(info.fonts[0].clone());
    assert!(info.validate().is_err());
    info.fonts.clear();
    assert!(info.validate().is_err());
    info.unicode.clear();
    info.width = 0;
    assert!(info.validate().is_err());
}

#[test]
fn required_replies_are_distinct_from_conditional_acks() {
    assert_eq!(DisplayCommand::GetLightMode.response_opcode(), Some(0x46));
    assert_eq!(DisplayCommand::UserGifCount.response_opcode(), Some(0xb6));
    assert_eq!(
        DisplayCommand::DeleteUserGif { item: 1 }.response_opcode(),
        Some(0xb6)
    );
    assert_eq!(
        DisplayCommand::GetArtworkInfo { index: 0 }.response_opcode(),
        Some(0x8e)
    );
    assert_eq!(
        DisplayCommand::GifTransfer(GifTransfer::Start { size: 1 }).response_opcode(),
        Some(0x8b)
    );
    assert_eq!(
        DisplayCommand::IndexedTransfer(IndexedTransfer::Start { size: 1, index: 0 })
            .response_opcode(),
        Some(0xb1)
    );
    assert_eq!(
        DisplayCommand::Big64Transfer(Big64Transfer::Start {
            size: 1,
            index: 0,
            file_id: EncodedArtworkId::from_u32_le(1)
        })
        .response_opcode(),
        Some(0x8d)
    );
    assert_eq!(
        DisplayCommand::UserGif(UserGif::Start(SavedArtworkKind::NormalImage)).response_opcode(),
        Some(0xb1)
    );
    assert_eq!(
        DisplayCommand::DrawMulti {
            screen: 0,
            color: rgb(),
            offsets: offsets()
        }
        .response_opcode(),
        None
    );
    assert_eq!(
        DisplayCommand::PhoneAnimation(chunk()).response_opcode(),
        None
    );
}
