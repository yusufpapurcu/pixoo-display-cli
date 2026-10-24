use divoom_bluetooth::{
    commands::{
        self,
        display::{DisplayCommand, EncodedImageBlock, SavedArtworkKind, UserGif},
    },
    profile::DeviceProfile,
    protocol::{self, BleEnvelope, BleLength, ClassicDialect, Packet},
    session::*,
    transport::Transport,
    Error,
};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Step {
    wire: Vec<u8>,
    response: Vec<u8>,
}
struct Mock {
    steps: VecDeque<Step>,
    input: VecDeque<u8>,
    partial: Vec<u8>,
    max_write: usize,
    reads: usize,
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    eof: bool,
    write_timeouts: usize,
    cancel_on_write: Option<CancellationToken>,
    cancel_on_read: Option<CancellationToken>,
}
impl Mock {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: steps.into(),
            input: VecDeque::new(),
            partial: Vec::new(),
            max_write: usize::MAX,
            reads: usize::MAX,
            writes: Arc::new(Mutex::new(Vec::new())),
            eof: false,
            write_timeouts: 0,
            cancel_on_write: None,
            cancel_on_read: None,
        }
    }
}
impl Transport for Mock {
    fn read(&mut self, bytes: &mut [u8], _: Duration) -> io::Result<usize> {
        if self.input.is_empty() {
            return if self.eof {
                Ok(0)
            } else {
                Err(io::ErrorKind::TimedOut.into())
            };
        }
        let n = bytes.len().min(self.input.len()).min(self.reads);
        for b in &mut bytes[..n] {
            *b = self.input.pop_front().unwrap();
        }
        if let Some(cancel) = &self.cancel_on_read {
            cancel.cancel();
        }
        Ok(n)
    }
    fn write(&mut self, bytes: &[u8], _: Duration) -> io::Result<usize> {
        if self.write_timeouts > 0 {
            self.write_timeouts -= 1;
            return Err(io::ErrorKind::TimedOut.into());
        }
        if self.max_write == 0 {
            return Ok(0);
        }
        let step = self.steps.front().expect("unexpected extra device write");
        let n = self
            .max_write
            .min(bytes.len())
            .min(step.wire.len() - self.partial.len());
        self.partial.extend_from_slice(&bytes[..n]);
        assert_eq!(
            self.partial,
            step.wire[..self.partial.len()],
            "wire differs from independently prepared transaction"
        );
        if self.partial.len() == step.wire.len() {
            self.writes
                .lock()
                .unwrap()
                .push(std::mem::take(&mut self.partial));
            let step = self.steps.pop_front().unwrap();
            self.input.extend(step.response);
            if let Some(cancel) = &self.cancel_on_write {
                cancel.cancel();
            }
        }
        Ok(n)
    }
}
fn p(op: u8, data: &[u8]) -> Packet {
    Packet::new(op, data.to_vec()).unwrap()
}
fn wire(p: &Packet) -> Vec<u8> {
    protocol::encode(p, ClassicDialect::PixooMax).unwrap()
}
fn response(op: u8, status: u8, data: &[u8]) -> Vec<u8> {
    let mut d = vec![op, status];
    d.extend(data);
    wire(&p(4, &d))
}
fn step(packet: Packet, reply: Vec<u8>) -> Step {
    Step {
        wire: wire(&packet),
        response: reply,
    }
}
fn config() -> SessionOptions {
    SessionOptions {
        pacing: Some(Duration::ZERO),
        poll_interval: Duration::from_millis(1),
        ..Default::default()
    }
}
fn session(mock: Mock) -> Session<Mock> {
    Session::new(mock, DeviceProfile::pixoo_max(), config()).unwrap()
}
fn timeout() -> Duration {
    Duration::from_millis(100)
}
fn token() -> CancellationToken {
    CancellationToken::default()
}
fn chunk(total: u32, id: u16, data: &[u8], op: u8) -> Packet {
    let mut d = vec![1];
    d.extend(total.to_le_bytes());
    d.extend(id.to_le_bytes());
    d.extend(data);
    p(op, &d)
}

#[test]
fn short_writes_fragmented_coalesced_replies_and_unrelated_events() {
    let mut r = response(9, 0x55, &[10]);
    r.extend(response(0x46, 0x55, &[5; 20]));
    let mut mock = Mock::new(vec![step(p(0x46, &[]), r)]);
    mock.max_write = 2;
    mock.reads = 1;
    let mut s = session(mock);
    let got = s
        .execute(
            &p(0x46, &[]),
            Some(&ReplyExpectation::opcode(0x46)),
            timeout(),
            &token(),
        )
        .unwrap();
    assert_eq!(got.delivery, Delivery::CommandAcknowledged);
    assert_eq!(got.reply.unwrap().data, vec![5; 20]);
    assert!(
        matches!(s.next_event(timeout(),&token()).unwrap(),Some(Event::Reply(r)) if r.opcode==9 && r.data==[10])
    );
}

#[test]
fn stale_matching_reply_never_satisfies_new_exchange_and_exact_queue_capacity_works() {
    let mut mock = Mock::new(vec![step(p(9, &[]), response(9, 0x55, &[3]))]);
    mock.input.extend(response(9, 0x55, &[15]));
    let mut options = config();
    options.event_capacity = 1;
    let mut s = Session::new(mock, DeviceProfile::pixoo_max(), options).unwrap();
    assert_eq!(
        s.execute(
            &p(9, &[]),
            Some(&ReplyExpectation::opcode(9)),
            timeout(),
            &token()
        )
        .unwrap()
        .reply
        .unwrap()
        .data,
        [3]
    );
    assert!(
        matches!(s.next_event(timeout(),&token()).unwrap(),Some(Event::Reply(r)) if r.data==[15])
    );
}

#[test]
fn read_only_priming_retries_ignored_first_request() {
    let mock = Mock::new(vec![
        step(p(0x46, &[]), vec![]),
        step(p(0x46, &[]), response(0x46, 0x55, &[5; 20])),
    ]);
    let mut s = session(mock);
    assert_eq!(
        s.prime(2, Duration::from_millis(2), &token())
            .unwrap()
            .opcode,
        0x46
    );
    assert!(!s.is_poisoned());
}

#[test]
fn negative_ack_and_zero_write_fail_and_poison_without_replay() {
    let mock = Mock::new(vec![step(p(9, &[]), response(9, 0, &[]))]);
    let mut s = session(mock);
    assert!(matches!(
        s.execute(
            &p(9, &[]),
            Some(&ReplyExpectation::opcode(9)),
            timeout(),
            &token()
        ),
        Err(Error::Rejected {
            opcode: 9,
            status: 0
        })
    ));
    assert!(s.is_poisoned());
    let mut mock = Mock::new(vec![step(p(9, &[]), vec![])]);
    mock.max_write = 0;
    assert!(
        matches!(session(mock).execute(&p(9,&[]),None,timeout(),&token()),Err(Error::Io(e)) if e.kind()==io::ErrorKind::WriteZero)
    );
}

#[test]
fn retryable_zero_progress_write_timeout_uses_remaining_absolute_deadline() {
    let mut mock = Mock::new(vec![step(p(9, &[]), response(9, 0x55, &[2]))]);
    mock.write_timeouts = 2;
    assert_eq!(
        session(mock)
            .execute(
                &p(9, &[]),
                Some(&ReplyExpectation::opcode(9)),
                timeout(),
                &token()
            )
            .unwrap()
            .reply
            .unwrap()
            .data,
        [2]
    );
}

#[test]
fn cancellation_eof_and_timeout_poison_interrupted_sessions() {
    let cancel = token();
    let mut mock = Mock::new(vec![step(p(9, &[]), vec![])]);
    mock.cancel_on_write = Some(cancel.clone());
    let mut s = session(mock);
    assert!(matches!(
        s.execute(
            &p(9, &[]),
            Some(&ReplyExpectation::opcode(9)),
            timeout(),
            &cancel
        ),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        s.execute(&p(9, &[]), None, timeout(), &token()),
        Err(Error::SessionPoisoned)
    ));
    let mut mock = Mock::new(vec![]);
    mock.eof = true;
    let mut s = session(mock);
    assert!(s.next_event(timeout(), &token()).is_err());
    assert!(s.is_poisoned());
    let mut s = session(Mock::new(vec![step(p(9, &[]), vec![])]));
    assert!(matches!(
        s.execute(
            &p(9, &[]),
            Some(&ReplyExpectation::opcode(9)),
            Duration::from_millis(2),
            &token()
        ),
        Err(Error::Timeout)
    ));
    assert!(s.is_poisoned());
}

#[test]
fn max_indexed_save_exact_start_chunks_end_and_captured_completion() {
    let native = vec![0xaa; 300];
    let mut start = vec![0];
    start.extend(300u32.to_le_bytes());
    start.push(2);
    let mock = Mock::new(vec![
        step(p(0x8c, &start), response(0xb1, 0x55, &[])),
        step(chunk(300, 0, &native[..256], 0x8c), vec![]),
        step(chunk(300, 1, &native[256..], 0x8c), vec![]),
        step(p(0x8c, &[2]), response(0x46, 0x55, &[5; 20])),
    ]);
    let got = session(mock)
        .transfer_file(
            FileDestination::Indexed(IndexedSlot::new(2)),
            &native,
            timeout(),
            &token(),
        )
        .unwrap();
    assert_eq!(got.bytes, 300);
    assert_eq!(got.chunks, 2);
    assert_eq!(got.retransmissions, 0);
    assert!(matches!(got.completion, TransferCompletion::CustomMode(_)));
}

#[test]
fn indexed_resends_are_bounded_and_out_of_range_requests_fail() {
    for requested in [0u16, 1u16] {
        let native = vec![0xaa; 10];
        let start = p(0x8c, &[0, 10, 0, 0, 0, 0]);
        let mut req = vec![1];
        req.extend(requested.to_le_bytes());
        let mut steps = vec![
            step(start, response(0xb1, 0x55, &[])),
            step(chunk(10, 0, &native, 0x8c), response(0x8c, 0x55, &req)),
        ];
        if requested == 0 {
            steps.extend([
                step(chunk(10, 0, &native, 0x8c), vec![]),
                step(p(0x8c, &[2]), response(0x46, 0x55, &[5; 20])),
            ]);
        }
        let mut s = session(Mock::new(steps));
        let got = s.transfer_file(
            FileDestination::Indexed(IndexedSlot::new(0)),
            &native,
            timeout(),
            &token(),
        );
        if requested == 0 {
            assert_eq!(got.unwrap().retransmissions, 1);
        } else {
            assert!(matches!(got, Err(Error::Protocol(_))));
            assert!(s.is_poisoned());
        }
    }
    let native = vec![0; 10];
    let mut options = config();
    options.transfer_retries = 0;
    let mock = Mock::new(vec![
        step(p(0x8c, &[0, 10, 0, 0, 0, 0]), response(0xb1, 0x55, &[])),
        step(
            chunk(10, 0, &native, 0x8c),
            response(0x8c, 0x55, &[1, 0, 0]),
        ),
    ]);
    let mut s = Session::new(mock, DeviceProfile::pixoo_max(), options).unwrap();
    assert!(matches!(
        s.transfer_file(
            FileDestination::Indexed(IndexedSlot::new(0)),
            &native,
            timeout(),
            &token()
        ),
        Err(Error::RetryLimit)
    ));
}

#[test]
fn stale_custom_status_cannot_complete_new_save() {
    let native = vec![0; 10];
    let mut mock = Mock::new(vec![
        step(p(0x8c, &[0, 10, 0, 0, 0, 0]), response(0xb1, 0x55, &[])),
        step(chunk(10, 0, &native, 0x8c), vec![]),
        step(p(0x8c, &[2]), vec![]),
    ]);
    mock.input.extend(response(0x46, 0x55, &[5; 20]));
    let observed = mock.writes.clone();
    let mut s = session(mock);
    assert!(matches!(
        s.transfer_file(
            FileDestination::Indexed(IndexedSlot::new(0)),
            &native,
            Duration::from_millis(3),
            &token()
        ),
        Err(Error::Timeout)
    ));
    assert_eq!(observed.lock().unwrap().len(), 3);
    assert!(s.is_poisoned());
}

#[test]
fn volatile_transfer_refusal_and_repair_require_a_fresh_barrier() {
    let native = vec![0; 10];
    let start = p(0x8b, &[0, 10, 0, 0, 0]);
    let mut refused = session(Mock::new(vec![step(
        start.clone(),
        response(0x8b, 0x55, &[0, 0]),
    )]));
    assert!(refused
        .transfer_file(FileDestination::Volatile, &native, timeout(), &token())
        .is_err());
    let mock = Mock::new(vec![
        step(start, response(0x8b, 0x55, &[0, 1])),
        step(chunk(10, 0, &native, 0x8b), vec![]),
        step(p(0x8b, &[2]), response(0x8b, 0x55, &[1, 0, 0])),
        step(p(0x46, &[]), response(0x46, 0x55, &[4; 20])),
        step(chunk(10, 0, &native, 0x8b), vec![]),
        step(p(0x8b, &[2]), vec![]),
        step(p(0x46, &[]), response(0x46, 0x55, &[5; 20])),
    ]);
    let got = session(mock)
        .transfer_file(FileDestination::Volatile, &native, timeout(), &token())
        .unwrap();
    assert_eq!(got.retransmissions, 1);
    assert!(matches!(got.completion,TransferCompletion::CommunicationBarrier(r) if r.data==[5;20]));
}

#[test]
fn legacy_b1_complete_frames_stay_intact() {
    let kind = SavedArtworkKind::NormalImage;
    let image = EncodedImageBlock::new(vec![0xaa, 3, 0]).unwrap();
    let a = DisplayCommand::UserGif(UserGif::Start(kind.clone()))
        .packet()
        .unwrap();
    let b = DisplayCommand::UserGif(UserGif::Frame(image.clone()))
        .packet()
        .unwrap();
    let c = DisplayCommand::UserGif(UserGif::End(kind.clone()))
        .packet()
        .unwrap();
    let mock = Mock::new(vec![
        step(a, response(0xb1, 0x55, &[])),
        step(b, vec![]),
        step(c, response(0xb1, 0x55, &[])),
    ]);
    assert_eq!(
        session(mock)
            .save_user_gif(kind, &[image], timeout(), &token())
            .unwrap()
            .chunks,
        1
    );
}

#[test]
fn ble_requires_matching_transport_ack_and_command_reply_in_either_order() {
    for reverse in [false, true] {
        let packet = p(9, &[]);
        let mut a = response(0x33, 0x55, &7u32.to_le_bytes());
        let b = response(9, 0x55, &[10]);
        let input = if reverse {
            let mut v = b;
            v.extend(a);
            v
        } else {
            a.extend(b);
            a
        };
        let mock = Mock::new(vec![Step {
            wire: BleEnvelope::command(&packet, Some(7))
                .encode(BleLength::IdentifierIncluded)
                .unwrap(),
            response: input,
        }]);
        let mut options = config();
        options.wire = WireProtocol::Ble {
            length: BleLength::IdentifierIncluded,
            initial_packet_number: 7,
            ack_timeout: Duration::from_millis(10),
            ack_retries: 1,
        };
        let mut s = Session::new(mock, DeviceProfile::pixoo_max(), options).unwrap();
        assert_eq!(
            s.execute(
                &packet,
                Some(&ReplyExpectation::opcode(9)),
                timeout(),
                &token()
            )
            .unwrap()
            .delivery,
            Delivery::CommandAcknowledged
        );
    }
}

#[test]
fn ble_retries_same_envelope_on_stale_ack_and_has_a_retry_limit() {
    let packet = p(0x74, &[50]);
    let bytes = BleEnvelope::command(&packet, Some(7))
        .encode(BleLength::IdentifierIncluded)
        .unwrap();
    let mut options = config();
    options.wire = WireProtocol::Ble {
        length: BleLength::IdentifierIncluded,
        initial_packet_number: 7,
        ack_timeout: Duration::from_millis(1),
        ack_retries: 1,
    };
    let mock = Mock::new(vec![
        Step {
            wire: bytes.clone(),
            response: response(0x33, 0x55, &6u32.to_le_bytes()),
        },
        Step {
            wire: bytes.clone(),
            response: response(0x33, 0x55, &7u32.to_le_bytes()),
        },
    ]);
    let mut s = Session::new(mock, DeviceProfile::pixoo_max(), options.clone()).unwrap();
    assert_eq!(
        s.execute(&packet, None, timeout(), &token())
            .unwrap()
            .delivery,
        Delivery::TransportAcknowledged
    );
    let mock = Mock::new(vec![
        Step {
            wire: bytes.clone(),
            response: vec![],
        },
        Step {
            wire: bytes,
            response: vec![],
        },
    ]);
    let mut s = Session::new(mock, DeviceProfile::pixoo_max(), options).unwrap();
    assert!(matches!(
        s.execute(&packet, None, timeout(), &token()),
        Err(Error::RetryLimit)
    ));
}

#[test]
fn stored_tab_and_calendar_use_distinct_checked_numbering() {
    assert!(commands::StoredTab::new(0).is_err());
    assert_eq!(
        commands::select_stored_tab(commands::StoredTab::new(3).unwrap()).unwrap()[1].payload(),
        [0x17, 2]
    );
    assert!(commands::date_time(2025, 2, 29, 0, 0, 0).is_err());
    assert_eq!(
        commands::date_time(2024, 2, 29, 23, 59, 58)
            .unwrap()
            .payload(),
        [24, 20, 2, 29, 23, 59, 58]
    );
}

#[test]
fn documented_statusless_work_mode_and_game_progress_keep_the_first_data_byte() {
    use divoom_bluetooth::commands::{
        schedule::{ScheduleCommand, TimeManagementControl},
        system::{SystemCommand, SystemReply, WorkMode},
    };
    let mut replies = wire(&p(4, &[0x17, 2, 3, 4]));
    replies.extend(wire(&p(4, &[0x13, 3])));
    let mock = Mock::new(vec![
        step(p(0x13, &[]), replies),
        step(p(0x57, &[1]), vec![]),
    ]);
    let mut s = session(mock);
    let got = s
        .execute_command(&SystemCommand::GetWorkMode, timeout(), &token())
        .unwrap();
    assert_eq!(got.delivery, Delivery::ResponseReceived);
    let event = got.notification.unwrap();
    assert!(matches!(
        SystemReply::decode_event(event.opcode, &event.data)
            .unwrap()
            .value,
        SystemReply::WorkMode(WorkMode::Sd)
    ));
    assert!(
        matches!(s.next_event(timeout(),&token()).unwrap(),Some(Event::Notification(n)) if n.opcode==0x17 && n.data==[2,3,4])
    );
    assert_eq!(
        s.execute_command(
            &ScheduleCommand::TimeManagementControl(TimeManagementControl::Disable),
            timeout(),
            &token()
        )
        .unwrap()
        .delivery,
        Delivery::Written
    );
}

#[test]
fn duplicate_statusless_responses_are_retained_as_events() {
    let mut input = wire(&p(4, &[0x13, 3]));
    input.extend(wire(&p(4, &[0x13, 4])));
    let mut s = session(Mock::new(vec![step(p(0x13, &[]), input)]));
    let receipt = s
        .execute_command(
            &commands::system::SystemCommand::GetWorkMode,
            timeout(),
            &token(),
        )
        .unwrap();
    assert_eq!(receipt.notification.unwrap().data, [3]);
    assert!(
        matches!(s.next_event(timeout(),&token()).unwrap(),Some(Event::Notification(n)) if n.data==[4])
    );
}

#[test]
fn sd_playlist_collects_all_records_and_accepts_an_empty_completion() {
    use commands::system::SystemCommand;
    for populated in [false, true] {
        let mut input = Vec::new();
        if populated {
            input.extend(response(7, 0x55, &[1, 0, 2, 0, 0x41, 0]));
            input.extend(response(7, 0x55, &[2, 0, 2, 0, 0x42, 0]));
        }
        input.extend(wire(&p(4, &[0x14])));
        let mut s = session(Mock::new(vec![step(p(7, &[1, 0, 2, 0]), input)]));
        assert!(s
            .execute_command(
                &SystemCommand::SdMusicList { start: 1, end: 2 },
                timeout(),
                &token()
            )
            .is_err());
        assert!(!s.is_poisoned());
        let items = s.sd_music_list(1, 2, timeout(), &token()).unwrap();
        assert_eq!(items.len(), if populated { 2 } else { 0 });
        if populated {
            assert_eq!(items[0].value.music_id, Some(1));
            assert_eq!(items[1].value.unicode_bytes, [0x42, 0]);
        }
    }
}

#[test]
fn artwork_inventory_collects_out_of_order_pages_and_rejects_missing_or_conflicting_ids() {
    for invalid in [false, true] {
        let mut input = response(0x8e, 0x55, &[1, 2, 2, 0, 1, 0, 1, 0, 0x22, 0, 0, 0]);
        if !invalid {
            input.extend(response(
                0x8e,
                0x55,
                &[1, 2, 2, 0, 0, 0, 1, 0, 0x11, 0, 0, 0],
            ));
        }
        input.extend(response(0x8e, 0x55, &[2, 2]));
        let mut s = session(Mock::new(vec![step(p(0x8e, &[2]), input)]));
        let got = s.artwork_inventory(2, timeout(), &token());
        if invalid {
            assert!(got.is_err());
            assert!(s.is_poisoned());
        } else {
            assert_eq!(
                got.unwrap()
                    .ids
                    .iter()
                    .map(|id| id.value())
                    .collect::<Vec<_>>(),
                [0x11, 0x22]
            );
        }
    }
}

#[test]
fn undocumented_final_ack_is_not_required_by_generic_storage_profiles() {
    let profile = DeviceProfile::large_canvas64();
    let destination = FileDestination::Big64 {
        index: 1,
        file_id: commands::display::EncodedArtworkId::from_u32_le(0x11223344),
    };
    let mock = Mock::new(vec![
        step(
            p(0x8d, &[0, 1, 0, 0, 0, 1, 0x44, 0x33, 0x22, 0x11]),
            response(0x8d, 0x55, &[]),
        ),
        step(chunk(1, 0, &[0xaa], 0x8d), vec![]),
        step(p(0x8d, &[2]), vec![]),
    ]);
    let mut s = Session::new(mock, profile, config()).unwrap();
    assert_eq!(
        s.transfer_file(destination, &[0xaa], timeout(), &token())
            .unwrap()
            .completion,
        TransferCompletion::TerminatorWritten
    );
}

#[test]
fn b1_completion_ack_is_an_explicit_device_policy() {
    let kind = SavedArtworkKind::NormalImage;
    let image = EncodedImageBlock::new(vec![0xaa, 3, 0]).unwrap();
    let steps = vec![
        step(
            DisplayCommand::UserGif(UserGif::Start(kind.clone()))
                .packet()
                .unwrap(),
            response(0xb1, 0x55, &[]),
        ),
        step(
            DisplayCommand::UserGif(UserGif::Frame(image.clone()))
                .packet()
                .unwrap(),
            vec![],
        ),
        step(
            DisplayCommand::UserGif(UserGif::End(kind.clone()))
                .packet()
                .unwrap(),
            response(0xb1, 0x55, &[]),
        ),
    ];
    let mut options = config();
    options.b1_completion = B1Completion::CommandAcknowledgment;
    let mut s = Session::new(Mock::new(steps), DeviceProfile::matrix16(), options).unwrap();
    assert!(matches!(
        s.save_user_gif(kind, &[image], timeout(), &token())
            .unwrap()
            .completion,
        TransferCompletion::CommandAcknowledged(_)
    ));
}

#[test]
fn complete_historical_max_save_capture_replays_exactly() {
    let mut steps: Vec<Step> = Vec::new();
    let mut native = Vec::new();
    for line in include_str!("fixtures/max-save-capture.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.first() == Some(&"TX")
            && (parts[1] == "start" || parts[1].starts_with("chunk-") || parts[1] == "end")
        {
            let bytes = (0..parts[2].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&parts[2][i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let packet = protocol::Decoder::new(ClassicDialect::PixooMax)
                .feed(&bytes)
                .unwrap()
                .remove(0);
            if packet.payload()[0] == 1 {
                native.extend_from_slice(&packet.payload()[7..]);
            }
            steps.push(Step {
                wire: bytes,
                response: vec![],
            });
        } else if parts.first() == Some(&"RX") {
            if let Some(last) = steps.last_mut() {
                last.response.extend(
                    (0..parts[1].len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&parts[1][i..i + 2], 16).unwrap()),
                );
            }
        }
    }
    assert_eq!(native.len(), 10822);
    assert_eq!(steps.len(), 45);
    #[cfg(feature = "media")]
    {
        let animation = divoom_bluetooth::media::Animation::decode_native(&native).unwrap();
        assert_eq!(animation.frames().len(), 24);
        assert_eq!(animation.encode_native().unwrap(), native);
    }
    let got = session(Mock::new(steps))
        .transfer_file(
            FileDestination::Indexed(IndexedSlot::new(0)),
            &native,
            timeout(),
            &token(),
        )
        .unwrap();
    assert_eq!(got.chunks, 43);
    assert!(matches!(got.completion, TransferCompletion::CustomMode(_)));
}

#[test]
fn cancellation_interrupts_nonblocking_preflight_drain_before_any_write() {
    let cancel = token();
    let mut mock = Mock::new(vec![]);
    mock.input.extend(wire(&p(0x49, &vec![0; 65532])));
    mock.reads = 1;
    mock.cancel_on_read = Some(cancel.clone());
    let writes = mock.writes.clone();
    let mut s = session(mock);
    assert!(matches!(
        s.execute(&p(9, &[]), None, timeout(), &cancel),
        Err(Error::Cancelled)
    ));
    assert!(writes.lock().unwrap().is_empty());
}

#[test]
fn unconfirmed_big64_termination_still_services_immediate_resend_requests() {
    let destination = FileDestination::Big64 {
        index: 0,
        file_id: commands::display::EncodedArtworkId::from_u32_le(1),
    };
    let data = chunk(1, 0, &[0xaa], 0x8d);
    let mock = Mock::new(vec![
        step(
            p(0x8d, &[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]),
            response(0x8d, 0x55, &[]),
        ),
        step(data.clone(), vec![]),
        step(p(0x8d, &[2]), response(0x8d, 0x55, &[1, 0, 0])),
        step(data, vec![]),
        step(p(0x8d, &[2]), vec![]),
    ]);
    let mut options = config();
    options.transfer_quiet = Duration::from_millis(1);
    let mut s = Session::new(mock, DeviceProfile::large_canvas64(), options).unwrap();
    let result = s
        .transfer_file(destination, &[0xaa], timeout(), &token())
        .unwrap();
    assert_eq!(result.retransmissions, 1);
    assert_eq!(result.completion, TransferCompletion::TerminatorWritten);
}

#[test]
fn b1_frame_rejection_aborts_before_end_for_both_completion_policies() {
    let kind = SavedArtworkKind::NormalImage;
    let image = EncodedImageBlock::new(vec![0xaa, 3, 0]).unwrap();
    for policy in [
        B1Completion::TerminatorWritten,
        B1Completion::CommandAcknowledgment,
    ] {
        let steps = vec![
            step(
                DisplayCommand::UserGif(UserGif::Start(kind.clone()))
                    .packet()
                    .unwrap(),
                response(0xb1, 0x55, &[]),
            ),
            step(
                DisplayCommand::UserGif(UserGif::Frame(image.clone()))
                    .packet()
                    .unwrap(),
                response(0xb1, 0, &[]),
            ),
        ];
        let mut options = config();
        options.b1_completion = policy;
        let mut s = Session::new(Mock::new(steps), DeviceProfile::matrix16(), options).unwrap();
        assert!(matches!(
            s.save_user_gif(
                kind.clone(),
                std::slice::from_ref(&image),
                timeout(),
                &token()
            ),
            Err(Error::Rejected {
                opcode: 0xb1,
                status: 0
            })
        ));
        assert!(s.is_poisoned());
    }
}
