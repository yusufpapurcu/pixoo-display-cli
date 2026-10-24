//! A single owner for requests and complete transfers. No operation retries a whole save.
use crate::{
    commands::{
        self,
        display::{
            Big64Transfer, DisplayCommand, EncodedArtworkId, EncodedImageBlock, FileChunk,
            GifTransfer, IndexedTransfer, SavedArtworkKind, UserGif,
        },
        Command, StoredTab,
    },
    profile::{DeviceFamily, DeviceProfile, StorageCompletion},
    protocol::{self, BleEnvelope, BleLength, Decoder, Notification, Packet, Reply},
    transport::Transport,
    Error, Result,
};
use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct ReplyRule {
    opcode: u8,
    prefix: Vec<u8>,
}
impl ReplyRule {
    pub fn new(opcode: u8, prefix: Vec<u8>) -> Self {
        Self { opcode, prefix }
    }
}
#[derive(Clone, Debug)]
pub struct ReplyExpectation {
    rules: Vec<ReplyRule>,
    notification: bool,
}
impl ReplyExpectation {
    pub fn opcode(opcode: u8) -> Self {
        Self {
            rules: vec![ReplyRule::new(opcode, vec![])],
            notification: false,
        }
    }
    pub fn prefix(opcode: u8, prefix: Vec<u8>) -> Self {
        Self {
            rules: vec![ReplyRule::new(opcode, prefix)],
            notification: false,
        }
    }
    pub fn notification(opcode: u8) -> Self {
        Self {
            rules: vec![ReplyRule::new(opcode, vec![])],
            notification: true,
        }
    }
    pub fn alternatives(rules: Vec<ReplyRule>) -> Result<Self> {
        if rules.is_empty() {
            Err(Error::Invalid("reply expectation needs an alternative"))
        } else {
            Ok(Self {
                rules,
                notification: false,
            })
        }
    }
    fn matches(&self, reply: &Reply) -> bool {
        !self.notification
            && self
                .rules
                .iter()
                .any(|r| r.opcode == reply.opcode && reply.data.starts_with(&r.prefix))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WireProtocol {
    Classic,
    Ble {
        length: BleLength,
        initial_packet_number: u32,
        ack_timeout: Duration,
        ack_retries: u8,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum B1Completion {
    TerminatorWritten,
    CommandAcknowledgment,
}
#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub wire: WireProtocol,
    pub event_capacity: usize,
    pub poll_interval: Duration,
    pub transfer_retries: u16,
    pub stream_response_limit: usize,
    pub stream_bytes_limit: usize,
    pub b1_completion: B1Completion,
    pub transfer_quiet: Duration,
    /// Override device pacing for a known transport or deterministic instruments.
    pub pacing: Option<Duration>,
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            wire: WireProtocol::Classic,
            event_capacity: 128,
            poll_interval: Duration::from_millis(20),
            transfer_retries: 16,
            stream_response_limit: 65537,
            stream_bytes_limit: 16 * 1024 * 1024,
            b1_completion: B1Completion::TerminatorWritten,
            transfer_quiet: Duration::from_millis(20),
            pacing: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Written,
    TransportAcknowledged,
    CommandAcknowledged,
    ResponseReceived,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub delivery: Delivery,
    pub reply: Option<Reply>,
    pub notification: Option<Notification>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Reply(Reply),
    Notification(Notification),
    Packet(Packet),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferCompletion {
    TerminatorWritten,
    CommandAcknowledged(Reply),
    CustomMode(Reply),
    CommunicationBarrier(Reply),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferReceipt {
    pub bytes: usize,
    pub chunks: usize,
    pub retransmissions: u16,
    pub completion: TransferCompletion,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtworkInventory {
    pub index: u8,
    pub ids: Vec<commands::display::ArtworkId>,
    pub extensions: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexedSlot(u8);
impl IndexedSlot {
    pub fn new(zero_based: u8) -> Self {
        Self(zero_based)
    }
    pub fn index(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug)]
pub enum FileDestination {
    Volatile,
    Indexed(IndexedSlot),
    Big64 {
        index: u8,
        file_id: EncodedArtworkId,
    },
}
struct PreparedFile {
    destination: FileDestination,
    start: Packet,
    end: Packet,
    chunks: Vec<Packet>,
    bytes: usize,
}
struct TransferScope {
    opcode: u8,
    start_epoch: u64,
    sent: usize,
}
impl FileDestination {
    fn opcode(&self) -> u8 {
        match self {
            Self::Volatile => 0x8b,
            Self::Indexed(_) => 0x8c,
            Self::Big64 { .. } => 0x8d,
        }
    }
    fn start(&self, size: u32) -> Result<Packet> {
        match self {
            Self::Volatile => DisplayCommand::GifTransfer(GifTransfer::Start { size }),
            Self::Indexed(slot) => DisplayCommand::IndexedTransfer(IndexedTransfer::Start {
                size,
                index: slot.0,
            }),
            Self::Big64 { index, file_id } => DisplayCommand::Big64Transfer(Big64Transfer::Start {
                size,
                index: *index,
                file_id: file_id.clone(),
            }),
        }
        .packet()
    }
    fn chunk(&self, chunk: FileChunk) -> Result<Packet> {
        match self {
            Self::Volatile => DisplayCommand::GifTransfer(GifTransfer::Data(chunk)),
            Self::Indexed(_) => DisplayCommand::IndexedTransfer(IndexedTransfer::Data(chunk)),
            Self::Big64 { .. } => DisplayCommand::Big64Transfer(Big64Transfer::Data(chunk)),
        }
        .packet()
    }
    fn end(&self) -> Result<Packet> {
        match self {
            Self::Volatile => DisplayCommand::GifTransfer(GifTransfer::End),
            Self::Indexed(_) => DisplayCommand::IndexedTransfer(IndexedTransfer::End),
            Self::Big64 { .. } => DisplayCommand::Big64Transfer(Big64Transfer::End),
        }
        .packet()
    }
}

pub struct Session<T> {
    transport: T,
    profile: DeviceProfile,
    options: SessionOptions,
    decoder: Decoder,
    pending: VecDeque<(u64, Packet)>,
    events: VecDeque<(u64, Event)>,
    epoch: u64,
    sequence: u32,
    last_write: Option<Instant>,
    poisoned: bool,
}
impl<T: Transport> Session<T> {
    pub fn new(transport: T, profile: DeviceProfile, options: SessionOptions) -> Result<Self> {
        if options.event_capacity == 0
            || options.stream_response_limit == 0
            || options.stream_bytes_limit == 0
            || options.poll_interval.is_zero()
            || options.poll_interval > Duration::from_millis(100)
        {
            return Err(Error::Invalid(
                "event capacity must be positive and polling interval must be 1ns..=100ms",
            ));
        }
        let sequence = match options.wire {
            WireProtocol::Classic => 0,
            WireProtocol::Ble {
                initial_packet_number,
                ack_timeout,
                ..
            } => {
                if ack_timeout.is_zero() {
                    return Err(Error::Invalid("BLE ACK timeout must be positive"));
                }
                initial_packet_number
            }
        };
        Ok(Self {
            transport,
            profile,
            decoder: Decoder::new(profile.dialect()),
            options,
            pending: VecDeque::new(),
            events: VecDeque::new(),
            epoch: 0,
            sequence,
            last_write: None,
            poisoned: false,
        })
    }
    pub fn profile(&self) -> DeviceProfile {
        self.profile
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    pub fn close(mut self) -> Result<()> {
        self.transport.close()?;
        Ok(())
    }
    fn deadline(timeout: Duration) -> Result<Instant> {
        Instant::now()
            .checked_add(timeout)
            .ok_or(Error::Invalid("timeout exceeds clock range"))
    }
    fn check(deadline: Instant, cancel: &CancellationToken) -> Result<Duration> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(Error::Timeout)
        } else {
            Ok(left)
        }
    }
    fn health(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::SessionPoisoned)
        } else {
            Ok(())
        }
    }
    fn guard<R>(&mut self, result: Result<R>) -> Result<R> {
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn queue(&mut self, (epoch, packet): (u64, Packet)) -> Result<()> {
        self.enqueue((epoch, Self::event(packet)?))
    }
    fn enqueue(&mut self, (epoch, event): (u64, Event)) -> Result<()> {
        if self.events.len() >= self.options.event_capacity {
            return Err(Error::Protocol("unsolicited event queue overflow"));
        }
        self.events.push_back((epoch, event));
        Ok(())
    }
    fn event(packet: Packet) -> Result<Event> {
        Ok(
            if packet.opcode() == 4
                && packet
                    .payload()
                    .first()
                    .is_some_and(|op| [0x13, 0x14, 0x17].contains(op))
            {
                Event::Notification(packet.notification()?)
            } else if packet.opcode() == 4 && packet.payload().len() >= 2 {
                Event::Reply(packet.reply()?)
            } else {
                Event::Packet(packet)
            },
        )
    }
    fn queue_pending(&mut self) -> Result<()> {
        while let Some(p) = self.pending.pop_front() {
            self.queue(p)?;
        }
        Ok(())
    }
    fn read_once(&mut self, timeout: Duration) -> Result<bool> {
        let mut data = [0u8; 4096];
        match self.transport.read(&mut data, timeout) {
            Ok(0) => {
                self.decoder.finish()?;
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Bluetooth peer closed").into())
            }
            Ok(n) if n <= data.len() => {
                self.pending.extend(
                    self.decoder
                        .feed(&data[..n])?
                        .into_iter()
                        .map(|p| (self.epoch, p)),
                );
                Ok(true)
            }
            Ok(_) => Err(Error::Protocol("transport returned an invalid read length")),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(e.into()),
        }
    }
    fn drain_available(&mut self, deadline: Instant, cancel: &CancellationToken) -> Result<()> {
        self.queue_pending()?;
        for _ in 0..65536usize.saturating_add(self.options.event_capacity) {
            Self::check(deadline, cancel)?;
            if !self.read_once(Duration::ZERO)? {
                return Ok(());
            }
            self.queue_pending()?;
        }
        Err(Error::Protocol("incoming stream did not settle"))
    }
    fn pace(&self, deadline: Instant, cancel: &CancellationToken) -> Result<()> {
        let delay = self.options.pacing.unwrap_or(self.profile.pacing());
        if let Some(last) = self.last_write {
            while last.elapsed() < delay {
                let left = Self::check(deadline, cancel)?;
                std::thread::sleep(
                    delay
                        .saturating_sub(last.elapsed())
                        .min(left)
                        .min(self.options.poll_interval),
                );
            }
        }
        Self::check(deadline, cancel)?;
        Ok(())
    }
    fn write_all(
        &mut self,
        bytes: &[u8],
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            let left = Self::check(deadline, cancel)?;
            match self
                .transport
                .write(&bytes[offset..], left.min(self.options.poll_interval))
            {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Bluetooth write made no progress",
                    )
                    .into())
                }
                Ok(n) if n <= bytes.len() - offset => offset += n,
                Ok(_) => return Err(Error::Protocol("transport returned invalid write length")),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue
                }
                Err(e) => return Err(e.into()),
            }
        }
        self.last_write = Some(Instant::now());
        Ok(())
    }
    fn exchange(
        &mut self,
        packet: &Packet,
        expected: Option<&ReplyExpectation>,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Receipt> {
        Self::check(deadline, cancel)?;
        self.pace(deadline, cancel)?;
        self.drain_available(deadline, cancel)?;
        while !self.decoder.is_empty() {
            let left = Self::check(deadline, cancel)?;
            self.read_once(left.min(self.options.poll_interval))?;
            self.queue_pending()?;
        }
        let (wire, id, ack_wait, max_retries) = match self.options.wire {
            WireProtocol::Classic => (
                protocol::encode(packet, self.profile.dialect())?,
                None,
                Duration::ZERO,
                0,
            ),
            WireProtocol::Ble {
                length,
                ack_timeout,
                ack_retries,
                ..
            } => {
                let id = self.sequence;
                self.sequence = self
                    .sequence
                    .checked_add(1)
                    .ok_or(Error::Protocol("BLE packet number exhausted; reconnect"))?;
                (
                    BleEnvelope::command(packet, Some(id)).encode(length)?,
                    Some(id),
                    ack_timeout,
                    ack_retries,
                )
            }
        };
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(Error::Protocol("session epoch exhausted"))?;
        self.write_all(&wire, deadline, cancel)?;
        let mut acknowledged = id.is_none();
        let mut reply = None;
        let mut notification = None;
        let mut retries = 0;
        let mut ack_deadline = Instant::now()
            .checked_add(ack_wait)
            .unwrap_or(deadline)
            .min(deadline);
        loop {
            if acknowledged && (expected.is_none() || reply.is_some() || notification.is_some()) {
                return Ok(Receipt {
                    delivery: if reply.is_some() {
                        Delivery::CommandAcknowledged
                    } else if notification.is_some() {
                        Delivery::ResponseReceived
                    } else if id.is_some() {
                        Delivery::TransportAcknowledged
                    } else {
                        Delivery::Written
                    },
                    reply,
                    notification,
                });
            }
            let left = Self::check(deadline, cancel)?;
            while let Some((epoch, p)) = self.pending.pop_front() {
                if let Some(expectation) = expected.filter(|e| e.notification) {
                    if p.opcode() == 4
                        && p.payload().first().is_some_and(|op| {
                            expectation
                                .rules
                                .iter()
                                .any(|r| r.opcode == *op && p.payload()[1..].starts_with(&r.prefix))
                        })
                    {
                        if notification.is_none() {
                            notification = Some(p.notification()?);
                        } else {
                            self.queue((epoch, p))?;
                        }
                        continue;
                    }
                }
                if p.opcode() == 4
                    && p.payload()
                        .first()
                        .is_some_and(|op| [0x13, 0x14, 0x17].contains(op))
                {
                    self.queue((epoch, p))?;
                    continue;
                }
                if p.opcode() != 4 || p.payload().len() < 2 {
                    self.queue((epoch, p))?;
                    continue;
                }
                let r = p.reply()?;
                if id.is_some() && r.opcode == 0x33 {
                    if r.data.len() != 4 {
                        return Err(Error::Protocol("malformed BLE transport ACK"));
                    }
                    let got = u32::from_le_bytes(
                        r.data
                            .as_slice()
                            .try_into()
                            .map_err(|_| Error::Protocol("short BLE ACK"))?,
                    );
                    if Some(got) == id {
                        r.ensure_success()?;
                        acknowledged = true;
                    } else {
                        self.queue((epoch, p))?;
                    }
                } else if expected.is_some_and(|v| v.matches(&r)) {
                    r.ensure_success()?;
                    if reply.is_none() {
                        reply = Some(r);
                    } else {
                        self.queue((epoch, p))?;
                    }
                } else {
                    self.queue((epoch, p))?;
                }
            }
            if acknowledged && (expected.is_none() || reply.is_some() || notification.is_some()) {
                continue;
            }
            if !acknowledged && Instant::now() >= ack_deadline {
                if retries >= max_retries {
                    return Err(Error::RetryLimit);
                }
                retries += 1;
                self.pace(deadline, cancel)?;
                self.write_all(&wire, deadline, cancel)?;
                ack_deadline = Instant::now()
                    .checked_add(ack_wait)
                    .unwrap_or(deadline)
                    .min(deadline);
            }
            let wait = if acknowledged {
                left
            } else {
                ack_deadline
                    .saturating_duration_since(Instant::now())
                    .min(left)
            };
            self.read_once(wait.min(self.options.poll_interval))?;
        }
    }
    /// Expert packet execution. For documented commands prefer `execute_command`.
    pub fn execute(
        &mut self,
        packet: &Packet,
        expected: Option<&ReplyExpectation>,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Receipt> {
        self.health()?;
        let deadline = Self::deadline(timeout)?;
        let result = self.exchange(packet, expected, deadline, cancel);
        self.guard(result)
    }
    pub fn execute_command<C: Command>(
        &mut self,
        command: &C,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Receipt> {
        if command.response_is_stream() {
            return Err(Error::Invalid("use the dedicated sd_music_list or artwork_inventory session method for streamed responses"));
        }
        let packet = command.packet()?;
        let expected = command.response_opcode().map(|op| {
            if command.response_is_notification() {
                ReplyExpectation::notification(op)
            } else {
                ReplyExpectation::opcode(op)
            }
        });
        self.execute(&packet, expected.as_ref(), timeout, cancel)
    }
    pub fn next_event(
        &mut self,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Option<Event>> {
        self.health()?;
        if let Some((_, event)) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let deadline = Self::deadline(timeout)?;
        let result = (|| loop {
            self.queue_pending()?;
            if let Some((_, event)) = self.events.pop_front() {
                return Ok(Some(event));
            }
            let left = match Self::check(deadline, cancel) {
                Err(Error::Timeout) => return Ok(None),
                other => other?,
            };
            self.read_once(left.min(self.options.poll_interval))?;
        })();
        self.guard(result)
    }
    /// Bounded, read-only startup queries handle Max connections that ignore the first request.
    pub fn prime(
        &mut self,
        attempts: u8,
        per_attempt: Duration,
        cancel: &CancellationToken,
    ) -> Result<Reply> {
        self.health()?;
        if attempts == 0 {
            return Err(Error::Invalid("priming requires at least one attempt"));
        }
        let packet = Packet::new(0x46, vec![])?;
        for _ in 0..attempts {
            match self.exchange(
                &packet,
                Some(&ReplyExpectation::opcode(0x46)),
                Self::deadline(per_attempt)?,
                cancel,
            ) {
                Ok(receipt) => {
                    return receipt
                        .reply
                        .ok_or(Error::Protocol("missing priming reply"))
                }
                Err(Error::Timeout) => continue,
                Err(e) => {
                    self.poisoned = true;
                    return Err(e);
                }
            }
        }
        self.poisoned = true;
        Err(Error::Timeout)
    }
    pub fn select_stored_tab(
        &mut self,
        tab: StoredTab,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Reply> {
        self.health()?;
        if self.profile.family() != DeviceFamily::PixooMax {
            return Err(Error::Unsupported(
                "stored tab selector is verified only for Pixoo-Max",
            ));
        }
        let packets = commands::select_stored_tab(tab)?;
        let deadline = Self::deadline(timeout)?;
        let result = (|| {
            for p in &packets {
                self.exchange(p, None, deadline, cancel)?;
            }
            let r = self
                .exchange(
                    &Packet::new(0x46, vec![])?,
                    Some(&ReplyExpectation::opcode(0x46)),
                    deadline,
                    cancel,
                )?
                .reply
                .ok_or(Error::Protocol("missing status"))?;
            if r.data.first() != Some(&5) {
                return Err(Error::Protocol("stored tab did not enter custom mode"));
            }
            Ok(r)
        })();
        self.guard(result)
    }
    /// Sends an already encoded native file using the documented start/data/end transaction.
    /// This accepts native encoder output, not GIF file bytes. It never retries a whole transaction.
    pub fn transfer_file(
        &mut self,
        destination: FileDestination,
        native: &[u8],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<TransferReceipt> {
        self.health()?;
        if matches!(destination, FileDestination::Big64 { .. })
            && !matches!(
                self.profile.family(),
                DeviceFamily::LargeCanvas64 | DeviceFamily::Custom
            )
        {
            return Err(Error::Unsupported(
                "8D requires an explicit large-canvas64 or custom device profile",
            ));
        }
        let size =
            u32::try_from(native.len()).map_err(|_| Error::Invalid("native file too large"))?;
        if let FileDestination::Indexed(slot) = destination {
            if self.profile.family() == DeviceFamily::PixooMax && slot.0 > 2 {
                return Err(Error::Invalid("Max has indexed slots0..2"));
            }
        }
        let start = destination.start(size)?;
        let end = destination.end()?;
        let chunks = native
            .chunks(256)
            .enumerate()
            .map(|(i, data)| {
                destination.chunk(FileChunk::new(
                    size,
                    u16::try_from(i).map_err(|_| Error::Invalid("too many file chunks"))?,
                    data.to_vec(),
                )?)
            })
            .collect::<Result<Vec<_>>>()?;
        let deadline = Self::deadline(timeout)?;
        let plan = PreparedFile {
            destination,
            start,
            end,
            chunks,
            bytes: native.len(),
        };
        let result = self.transfer_inner(&plan, deadline, cancel);
        self.guard(result)
    }
    fn transfer_inner(
        &mut self,
        plan: &PreparedFile,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<TransferReceipt> {
        let PreparedFile {
            destination,
            start,
            end,
            chunks,
            bytes,
        } = plan;
        let bytes = *bytes;
        let op = destination.opcode();
        let ready_op = if op == 0x8c { 0xb1 } else { op };
        let ready = self
            .exchange(
                start,
                Some(&ReplyExpectation::opcode(ready_op)),
                deadline,
                cancel,
            )?
            .reply
            .ok_or(Error::Protocol("missing transfer readiness"))?;
        let start_epoch = self.epoch;
        if op == 0x8b && (ready.data.len() < 2 || ready.data[..2] != [0, 1]) {
            return Err(Error::Protocol(
                "device refused or malformed transfer readiness",
            ));
        }
        let mut retransmissions = 0;
        for (sent, p) in chunks.iter().enumerate() {
            self.exchange(p, None, deadline, cancel)?;
            self.drain_available(deadline, cancel)?;
            self.service_retransmissions(
                TransferScope {
                    opcode: op,
                    start_epoch,
                    sent: sent + 1,
                },
                chunks,
                &mut retransmissions,
                deadline,
                cancel,
            )?;
        }
        self.exchange(end, None, deadline, cancel)?;
        let mut end_epoch = self.epoch;
        if op != 0x8b && self.profile.indexed_completion() == StorageCompletion::TerminatorWritten {
            let mut quiet_until = Instant::now()
                .checked_add(self.options.transfer_quiet)
                .unwrap_or(deadline)
                .min(deadline);
            loop {
                self.queue_pending()?;
                if self.service_retransmissions(
                    TransferScope {
                        opcode: op,
                        start_epoch,
                        sent: chunks.len(),
                    },
                    chunks,
                    &mut retransmissions,
                    deadline,
                    cancel,
                )? {
                    self.exchange(end, None, deadline, cancel)?;
                    quiet_until = Instant::now()
                        .checked_add(self.options.transfer_quiet)
                        .unwrap_or(deadline)
                        .min(deadline);
                }
                if Instant::now() >= quiet_until {
                    Self::check(deadline, cancel)?;
                    break;
                }
                let left = Self::check(deadline, cancel)?;
                self.read_once(
                    quiet_until
                        .saturating_duration_since(Instant::now())
                        .min(left)
                        .min(self.options.poll_interval),
                )?;
            }
            return Ok(TransferReceipt {
                bytes,
                chunks: chunks.len(),
                retransmissions,
                completion: TransferCompletion::TerminatorWritten,
            });
        }
        if op == 0x8b {
            // No completion ACK is defined for 8B. A status query provides a communication barrier.
            self.exchange(&Packet::new(0x46, vec![])?, None, deadline, cancel)?;
        }
        loop {
            self.queue_pending()?;
            let replayed = self.service_retransmissions(
                TransferScope {
                    opcode: op,
                    start_epoch,
                    sent: chunks.len(),
                },
                chunks,
                &mut retransmissions,
                deadline,
                cancel,
            )?;
            if replayed {
                self.exchange(end, None, deadline, cancel)?;
                end_epoch = self.epoch;
                if op == 0x8b {
                    self.exchange(&Packet::new(0x46, vec![])?, None, deadline, cancel)?;
                }
            }
            let complete_op = match destination {
                FileDestination::Volatile => 0x46,
                FileDestination::Big64 { .. } | FileDestination::Indexed(_) => {
                    match self.profile.indexed_completion() {
                        StorageCompletion::B1Acknowledgment => 0xb1,
                        StorageCompletion::CustomModeStatus => 0x46,
                        StorageCompletion::TransferOpcode => op,
                        StorageCompletion::TerminatorWritten => {
                            return Err(Error::Protocol("completion policy already handled"))
                        }
                    }
                }
            };
            if let Some(i) = self.events.iter().position(|(epoch, e)| {
                *epoch >= end_epoch && matches!(e,Event::Reply(r) if r.opcode==complete_op)
            }) {
                if let Some((_, Event::Reply(r))) = self.events.remove(i) {
                    r.ensure_success()?;
                    let completion = if op == 0x8b {
                        TransferCompletion::CommunicationBarrier(r)
                    } else if complete_op == 0x46 {
                        commands::display::LightSettings::decode(&r)?;
                        if r.data.first() != Some(&5) {
                            return Err(Error::Protocol(
                                "save completion status is not custom mode",
                            ));
                        }
                        TransferCompletion::CustomMode(r)
                    } else {
                        TransferCompletion::CommandAcknowledged(r)
                    };
                    return Ok(TransferReceipt {
                        bytes,
                        chunks: chunks.len(),
                        retransmissions,
                        completion,
                    });
                }
            }
            let left = Self::check(deadline, cancel)?;
            self.read_once(left.min(self.options.poll_interval))?;
        }
    }
    fn service_retransmissions(
        &mut self,
        scope: TransferScope,
        chunks: &[Packet],
        retries: &mut u16,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<bool> {
        let TransferScope {
            opcode,
            start_epoch,
            sent,
        } = scope;
        let mut replayed = false;
        while let Some(i) = self.events.iter().position(|(epoch,e)| *epoch >= start_epoch && matches!(e,Event::Reply(r) if r.opcode==opcode && (r.status!=0x55 || r.data.first()==Some(&1)))) {
            if let Some((_,Event::Reply(r))) = self.events.remove(i) {
                r.ensure_success()?;
                if r.data.len() != 3 { return Err(Error::Protocol("malformed retransmission request")); }
                let id = usize::from(u16::from_le_bytes([r.data[1],r.data[2]]));
                if id >= sent || id >= chunks.len() { return Err(Error::Protocol("requested chunk was not sent")); }
                if *retries >= self.options.transfer_retries { return Err(Error::RetryLimit); }
                *retries += 1;
                self.exchange(&chunks[id],None,deadline,cancel)?;
                replayed = true;
            }
        }
        Ok(replayed)
    }
    /// Legacy B1 frame saving keeps complete frames intact instead of splitting them into file chunks.
    pub fn save_user_gif(
        &mut self,
        kind: SavedArtworkKind,
        frames: &[EncodedImageBlock],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<TransferReceipt> {
        self.health()?;
        if frames.is_empty() {
            return Err(Error::Invalid("B1 transfer needs frames"));
        }
        let start = DisplayCommand::UserGif(UserGif::Start(kind.clone())).packet()?;
        let end = DisplayCommand::UserGif(UserGif::End(kind)).packet()?;
        let packets = frames
            .iter()
            .map(|f| DisplayCommand::UserGif(UserGif::Frame(f.clone())).packet())
            .collect::<Result<Vec<_>>>()?;
        let deadline = Self::deadline(timeout)?;
        let result = (|| {
            self.exchange(
                &start,
                Some(&ReplyExpectation::opcode(0xb1)),
                deadline,
                cancel,
            )?;
            let start_epoch = self.epoch;
            for p in &packets {
                self.exchange(p, None, deadline, cancel)?;
                self.drain_available(deadline, cancel)?;
                self.check_b1_replies(start_epoch)?;
            }
            let expectation = if self.options.b1_completion == B1Completion::CommandAcknowledgment {
                Some(ReplyExpectation::opcode(0xb1))
            } else {
                None
            };
            let receipt = self.exchange(&end, expectation.as_ref(), deadline, cancel)?;
            if expectation.is_none() {
                let quiet_until = Instant::now()
                    .checked_add(self.options.transfer_quiet)
                    .unwrap_or(deadline)
                    .min(deadline);
                loop {
                    self.queue_pending()?;
                    self.check_b1_replies(start_epoch)?;
                    Self::check(deadline, cancel)?;
                    if Instant::now() >= quiet_until {
                        break;
                    }
                    self.read_once(
                        quiet_until
                            .saturating_duration_since(Instant::now())
                            .min(self.options.poll_interval),
                    )?;
                }
            }
            self.drain_available(deadline, cancel)?;
            self.check_b1_replies(start_epoch)?;
            let completion = match receipt.reply {
                Some(r) => TransferCompletion::CommandAcknowledged(r),
                None => TransferCompletion::TerminatorWritten,
            };
            Ok(TransferReceipt {
                bytes: frames.iter().map(|f| f.bytes().len()).sum(),
                chunks: frames.len(),
                retransmissions: 0,
                completion,
            })
        })();
        self.guard(result)
    }
    fn check_b1_replies(&mut self, start_epoch: u64) -> Result<()> {
        while let Some(index) = self.events.iter().position(|(epoch, e)| {
            *epoch >= start_epoch && matches!(e,Event::Reply(r) if r.opcode==0xb1)
        }) {
            if let Some((_, Event::Reply(reply))) = self.events.remove(index) {
                reply.ensure_success()?;
            }
        }
        Ok(())
    }
    /// Sends a prepared packet series under one borrow, then queries actual display status.
    pub fn display_packets(
        &mut self,
        packets: &[Packet],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Reply> {
        self.health()?;
        if packets.is_empty() {
            return Err(Error::Invalid("display sequence must be nonempty"));
        }
        let deadline = Self::deadline(timeout)?;
        let result = (|| {
            for p in packets {
                self.exchange(p, None, deadline, cancel)?;
            }
            self.exchange(
                &Packet::new(0x46, vec![])?,
                Some(&ReplyExpectation::opcode(0x46)),
                deadline,
                cancel,
            )?
            .reply
            .ok_or(Error::Protocol("missing display barrier"))
        })();
        self.guard(result)
    }
    fn stream_event(
        &mut self,
        epoch: u64,
        predicate: impl Fn(&Event) -> bool,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Event> {
        loop {
            Self::check(deadline, cancel)?;
            if let Some(i) = self
                .events
                .iter()
                .position(|(e, event)| *e >= epoch && predicate(event))
            {
                return self
                    .events
                    .remove(i)
                    .map(|(_, e)| e)
                    .ok_or(Error::Protocol("stream event disappeared"));
            }
            if let Some((received, p)) = self.pending.pop_front() {
                let event = Self::event(p)?;
                if received >= epoch && predicate(&event) {
                    return Ok(event);
                }
                self.enqueue((received, event))?;
                continue;
            }
            let left = Self::check(deadline, cancel)?;
            self.read_once(left.min(self.options.poll_interval))?;
        }
    }
    /// Collects all track records through the status-less 14 terminator, including an empty list.
    pub fn sd_music_list(
        &mut self,
        start: u16,
        end: u16,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Vec<commands::system::Decoded<commands::system::SdName>>> {
        use commands::system::{Decoded, SystemCommand, SystemReply};
        self.health()?;
        let packet = SystemCommand::SdMusicList { start, end }.packet()?;
        let deadline = Self::deadline(timeout)?;
        let result = (|| {
            self.exchange(&packet, None, deadline, cancel)?;
            let epoch = self.epoch;
            let mut tracks = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            let mut bytes = 0usize;
            for _ in 0..self.options.stream_response_limit {
                let event = self.stream_event(
                    epoch,
                    |e| {
                        matches!(e,Event::Reply(r) if r.opcode==7)
                            || matches!(e,Event::Notification(n) if n.opcode==0x14)
                    },
                    deadline,
                    cancel,
                )?;
                match event {
                    Event::Notification(_) => return Ok(tracks),
                    Event::Reply(r) => {
                        bytes = bytes
                            .checked_add(r.data.len())
                            .ok_or(Error::Protocol("playlist byte count overflow"))?;
                        if bytes > self.options.stream_bytes_limit {
                            return Err(Error::Protocol("playlist exceeds byte limit"));
                        }
                        let decoded = SystemReply::decode(&r)?;
                        if let SystemReply::SdName(name) = decoded.value {
                            let id = name
                                .music_id
                                .ok_or(Error::Protocol("playlist item lacks track ID"))?;
                            if id < start || id > end || !seen.insert(id) {
                                return Err(Error::Protocol(
                                    "playlist track ID is duplicate or outside requested range",
                                ));
                            }
                            tracks.push(Decoded {
                                value: name,
                                extension: decoded.extension,
                            });
                        } else {
                            return Err(Error::Protocol("invalid playlist record"));
                        }
                    }
                    _ => return Err(Error::Protocol("invalid playlist event")),
                }
            }
            Err(Error::Protocol("playlist exceeds response limit"))
        })();
        self.guard(result)
    }
    /// Collects 8E pages, validates their ranges and requires a matching control2 end record.
    pub fn artwork_inventory(
        &mut self,
        index: u8,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<ArtworkInventory> {
        use commands::display::ArtworkInfoReply;
        self.health()?;
        let packet = DisplayCommand::GetArtworkInfo { index }.packet()?;
        let deadline = Self::deadline(timeout)?;
        let result = (|| {
            self.exchange(&packet, None, deadline, cancel)?;
            let epoch = self.epoch;
            let mut ids: Option<Vec<Option<commands::display::ArtworkId>>> = None;
            let mut extensions = Vec::new();
            let mut bytes = 0usize;
            for _ in 0..self.options.stream_response_limit {
                let event = self.stream_event(epoch,|e| matches!(e,Event::Reply(r) if r.opcode==0x8e && (r.data.len()<2 || r.data.get(1)==Some(&index))),deadline,cancel)?;
                if let Event::Reply(r) = event {
                    bytes = bytes
                        .checked_add(r.data.len())
                        .ok_or(Error::Protocol("inventory byte count overflow"))?;
                    if bytes > self.options.stream_bytes_limit {
                        return Err(Error::Protocol("inventory exceeds byte limit"));
                    }
                    match ArtworkInfoReply::decode(&r)? {
                        ArtworkInfoReply::Files {
                            total,
                            offset,
                            ids: page,
                            extensions: tail,
                            ..
                        } => {
                            let slots = ids.get_or_insert_with(|| vec![None; usize::from(total)]);
                            if slots.len() != usize::from(total) {
                                return Err(Error::Protocol(
                                    "inventory total changed between pages",
                                ));
                            }
                            for (i, id) in page.into_iter().enumerate() {
                                let slot = &mut slots[usize::from(offset) + i];
                                if slot.is_some_and(|old| old != id) {
                                    return Err(Error::Protocol("inventory pages conflict"));
                                }
                                *slot = Some(id);
                            }
                            extensions.push(tail);
                        }
                        ArtworkInfoReply::End {
                            extensions: tail, ..
                        } => {
                            extensions.push(tail);
                            let complete = ids
                                .unwrap_or_default()
                                .into_iter()
                                .map(|id| {
                                    id.ok_or(Error::Protocol(
                                        "inventory ended before all IDs arrived",
                                    ))
                                })
                                .collect::<Result<Vec<_>>>()?;
                            return Ok(ArtworkInventory {
                                index,
                                ids: complete,
                                extensions,
                            });
                        }
                        ArtworkInfoReply::Unknown { .. } => {
                            return Err(Error::Protocol("unknown inventory control"))
                        }
                    }
                }
            }
            Err(Error::Protocol("inventory exceeds response limit"))
        })();
        self.guard(result)
    }
}
