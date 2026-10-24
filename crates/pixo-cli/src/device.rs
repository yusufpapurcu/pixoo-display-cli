use crate::{
    config::{controller_address, Device, Lease},
    error::{CliError, Result},
    media::Asset,
};
use divoom_bluetooth::{
    commands::{
        display::{DisplayCommand, LightMode, LightSettings},
        system::SystemCommand,
        Percent, StoredTab,
    },
    media::{chunk_native, MediaProfile},
    profile::DeviceProfile,
    protocol::{Packet, Reply},
    session::{
        CancellationToken, FileDestination, IndexedSlot, Session, SessionOptions,
        TransferCompletion,
    },
    transport::Transport,
};
use serde_json::{json, Value};
use std::{
    net::TcpStream,
    time::{Duration, Instant},
};

pub enum Action {
    Status,
    Display { asset: Asset, packets: Vec<Packet> },
    Save { asset: Asset, tab: u8 },
    Stored(u8),
    Brightness(Option<u8>),
    Mode(LightMode),
}
pub struct Prepared {
    pub action: Action,
    pub brightness: Option<u8>,
}
impl Action {
    pub fn display(asset: Asset) -> Result<Self> {
        let packets = if asset.animation.frames().len() == 1 {
            vec![asset.animation.frames()[0].static_packet()?]
        } else {
            chunk_native(&asset.native, MediaProfile::PixooMax)?
        };
        Ok(Self::Display { asset, packets })
    }
}
impl Prepared {
    pub fn media(&self) -> Option<&Asset> {
        match &self.action {
            Action::Display { asset, .. } | Action::Save { asset, .. } => Some(asset),
            _ => None,
        }
    }
    pub fn describe(&self) -> Value {
        let action = match &self.action {
            Action::Status => {
                json!({"operation":"status","expected_completion":"response_received"})
            }
            Action::Display { asset, .. } => {
                json!({"operation":"display","media":asset.description,"expected_completion":"communication_barrier"})
            }
            Action::Save { asset, tab } => {
                json!({"operation":"save","tab":tab,"replaces_artwork":true,"execution_requires_replace":true,
                "media":asset.description,"expected_completion":"custom_mode_status","power_cycle_retention":"unverified"})
            }
            Action::Stored(tab) => {
                json!({"operation":"stored","tab":tab,"expected_completion":"custom_mode_status"})
            }
            Action::Brightness(value) => {
                json!({"operation":"brightness","requested":value,"expected_completion":"response_received"})
            }
            Action::Mode(_) => {
                json!({"operation":"mode","expected_completion":"response_received"})
            }
        };
        json!({"dry_run":true,"profile":"pixoo-max","size":32,"brightness":self.brightness,"plan":action})
    }
    pub fn default_timeout(&self) -> Duration {
        Duration::from_secs(if self.media().is_some() { 60 } else { 10 })
    }
    fn mutates(&self) -> bool {
        self.brightness.is_some()
            || !matches!(self.action, Action::Status | Action::Brightness(None))
    }
}

pub fn execute(
    device: &Device,
    prepared: Prepared,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Value> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        use divoom_bluetooth::transport::{Rfcomm, RfcommChannel};
        let deadline = Instant::now() + timeout;
        let _lease = Lease::device(device)?;
        check_cancel(cancel)?;
        if let Some(endpoint) = &device.controller {
            match TcpStream::connect_timeout(&controller_address(endpoint)?,remaining(deadline)?.min(Duration::from_millis(300))) {
                Ok(_)=>return Err(CliError::new("busy",3,"the configured legacy controller endpoint is listening; release its device connection first")),
                Err(e) if e.kind()==std::io::ErrorKind::ConnectionRefused=>{},
                Err(e)=>return Err(CliError::new("ownership_check",3,format!("cannot rule out a legacy controller: {e}"))),
            }
        }
        let address = device.address.parse()?;
        let transport = connection_outcome(
            Rfcomm::connect(
                address,
                RfcommChannel::new(device.channel)?,
                remaining(deadline)?.min(Duration::from_secs(5)),
            ),
            cancel,
        )?;
        let mut session = Session::new(
            transport,
            DeviceProfile::pixoo_max(),
            SessionOptions::default(),
        )?;
        let outcome = (|| {
            check_cancel(cancel)?;
            let budget = remaining(deadline)?;
            let per_attempt = (budget / 3).min(Duration::from_secs(1));
            if per_attempt.is_zero() {
                return Err(CliError::new(
                    "timeout",
                    7,
                    "device budget exhausted before priming",
                ));
            }
            let primed = session.prime(3, per_attempt, cancel)?;
            execute_session(&mut session, prepared, primed, deadline, cancel)
        })();
        let cleanup = session.close();
        match (outcome, cleanup) {
            (Ok(mut result), Err(e)) => {
                result["cleanup_warning"] = json!(e.to_string());
                Ok(result)
            }
            (outcome, _) => outcome,
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (device, prepared, timeout, cancel);
        Err(CliError::unsupported("native Classic Bluetooth is available only on Windows and Linux; macOS Classic is not implemented"))
    }
}

fn connection_outcome<T: Transport>(
    mut connected: std::io::Result<T>,
    cancel: &CancellationToken,
) -> Result<T> {
    if let Err(mut cancelled) = check_cancel(cancel) {
        if let Ok(link) = &mut connected {
            if let Err(cleanup) = link.close() {
                cancelled
                    .message
                    .push_str(&format!("; connection cleanup failed: {cleanup}"));
            }
        }
        return Err(cancelled);
    }
    connected.map_err(|error| CliError::new("connection", 5, error.to_string()))
}

fn execute_session<T: Transport>(
    session: &mut Session<T>,
    prepared: Prepared,
    primed: Reply,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Result<Value> {
    let mut completed = Vec::new();
    let mutating = prepared.mutates();
    let result = (|| -> Result<Value> {
        if let Some(value) = prepared.brightness {
            completed.push(set_brightness(session, value, deadline, cancel)?);
        }
        let mut result = match prepared.action {
            Action::Status | Action::Brightness(None) => {
                json!({"status":status(&primed)?,"completion":"response_received"})
            }
            Action::Brightness(Some(value)) => set_brightness(session, value, deadline, cancel)?,
            Action::Display { asset, packets } => {
                let reply = session.display_packets(&packets, remaining(deadline)?, cancel)?;
                json!({"media":asset.description,"status":status(&reply)?,"completion":"communication_barrier"})
            }
            Action::Stored(tab) => {
                let reply = session.select_stored_tab(
                    StoredTab::new(tab)?,
                    remaining(deadline)?,
                    cancel,
                )?;
                json!({"tab":tab,"status":status(&reply)?,"completion":"custom_mode_status"})
            }
            Action::Save { asset, tab } => {
                let receipt = session.transfer_file(
                    FileDestination::Indexed(IndexedSlot::new(tab - 1)),
                    &asset.native,
                    remaining(deadline)?,
                    cancel,
                )?;
                let completion = match receipt.completion {
                    TransferCompletion::TerminatorWritten => "terminator_written",
                    TransferCompletion::CommandAcknowledged(_) => "command_acknowledged",
                    TransferCompletion::CustomMode(_) => "custom_mode_status",
                    TransferCompletion::CommunicationBarrier(_) => "communication_barrier",
                };
                json!({"tab":tab,"bytes":receipt.bytes,"chunks":receipt.chunks,"retransmissions":receipt.retransmissions,
                    "completion":completion,"power_cycle_retention":"unverified","media":asset.description})
            }
            Action::Mode(mode) => {
                let expected = match mode {
                    LightMode::UserDefined => 5,
                    LightMode::Light { .. } => 1,
                    _ => return Err(CliError::unsupported("this mode is not exposed by the CLI")),
                };
                session.execute_command(
                    &DisplayCommand::SetLightMode(mode),
                    remaining(deadline)?,
                    cancel,
                )?;
                let reply = query_status(session, deadline, cancel)?;
                if reply.data.first() != Some(&expected) {
                    return Err(CliError::new(
                        "device_result",
                        6,
                        "device mode did not match the requested mode",
                    ));
                }
                json!({"status":status(&reply)?,"completion":"response_received"})
            }
        };
        result["completed_steps"] = json!(completed);
        Ok(result)
    })();
    result.map_err(|mut e| {
        e.uncertain = mutating;
        e.completed = completed;
        e
    })
}

fn set_brightness<T: Transport>(
    session: &mut Session<T>,
    value: u8,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Result<Value> {
    session.execute_command(
        &SystemCommand::Brightness(Percent::new(value)?),
        remaining(deadline)?,
        cancel,
    )?;
    let reply = query_status(session, deadline, cancel)?;
    let observed = LightSettings::decode(&reply)?.system_brightness;
    if value != observed {
        return Err(CliError::new(
            "device_result",
            6,
            format!("requested brightness {value}, observed {observed}"),
        ));
    }
    Ok(
        json!({"operation":"brightness","requested":value,"observed":observed,"completion":"response_received"}),
    )
}
fn query_status<T: Transport>(
    session: &mut Session<T>,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Result<Reply> {
    session
        .execute_command(&DisplayCommand::GetLightMode, remaining(deadline)?, cancel)?
        .reply
        .ok_or_else(|| CliError::new("protocol", 6, "missing display status"))
}
fn status(reply: &Reply) -> Result<Value> {
    let s = LightSettings::decode(reply)?;
    let code = reply.data[0];
    let label = match code {
        0 => "clock",
        1 => "night_light",
        2 => "hot",
        3 => "vj",
        4 => "music_eq_or_flag",
        5 => "custom",
        6 => "lyric",
        _ => "unknown",
    };
    Ok(
        json!({"mode_code":code,"documented_mode":label,"physical_mode_meaning":"not_verified",
        "system_brightness":s.system_brightness,"light_brightness":s.brightness,"color":[s.color.r,s.color.g,s.color.b],
        "temperature_mode":s.temperature_mode,"time_format":s.time_format,"time_color":[s.time_color.r,s.time_color.g,s.time_color.b],
        "time_display_mode":s.time_display_mode,"time_checkboxes":s.time_checkboxes,"light_switch":s.light_switch,
        "light_selection":s.light_selection,"music_selection":s.music_selection,"vj_selection":s.vj_selection,"extensions":s.extensions}),
    )
}
fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(CliError::new(
            "cancelled",
            130,
            "cancelled before device operation",
        ))
    } else {
        Ok(())
    }
}
fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| CliError::new("timeout", 7, "overall device deadline expired"))
}

pub fn capabilities() -> Value {
    json!({"profile":"pixoo-max","size":32,"transport":"classic","platforms":{"windows":"read_only_hardware_verified","linux":"compiled_only","macos":"unsupported"},
        "display_status":"rust_read_only_verified","image_gif_encoding":"fixture_verified","save_and_recall":"inherited_max_observations",
        "power_cycle_retention":"unverified","native_64_encoding":"unresolved","native_87_text_encoding":"unresolved",
        "text":"host_rendered_printable_ascii","classic_discovery":false,"ble_cli":false,
        "advanced_families":"library_only_model_support_unverified"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use divoom_bluetooth::protocol::{self, ClassicDialect, Decoder};
    use std::{
        collections::VecDeque,
        io,
        sync::{Arc, Mutex},
    };

    #[derive(Debug)]
    struct ClosingConnection(Arc<std::sync::atomic::AtomicBool>);
    impl Transport for ClosingConnection {
        fn read(&mut self, _: &mut [u8], _: Duration) -> io::Result<usize> {
            panic!("cancelled connection must not read application bytes")
        }
        fn write(&mut self, _: &[u8], _: Duration) -> io::Result<usize> {
            panic!("cancelled connection must not write application bytes")
        }
        fn close(&mut self) -> io::Result<()> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn connection_failure_distinguishes_cancelled_attempts() {
        let cancel = CancellationToken::default();
        let error =
            connection_outcome::<ClosingConnection>(Err(io::ErrorKind::TimedOut.into()), &cancel)
                .unwrap_err();
        assert_eq!(error.code, 5);
        cancel.cancel();
        let error =
            connection_outcome::<ClosingConnection>(Err(io::ErrorKind::TimedOut.into()), &cancel)
                .unwrap_err();
        assert_eq!(error.code, 130);
        assert!(!error.uncertain);
    }

    #[test]
    fn cancellation_closes_a_connection_that_succeeded_before_any_application_io() {
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = CancellationToken::default();
        cancel.cancel();
        let error = connection_outcome(Ok(ClosingConnection(closed.clone())), &cancel).unwrap_err();
        assert_eq!(error.code, 130);
        assert!(!error.uncertain);
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
    }

    struct DeviceDouble {
        decoder: Decoder,
        input: VecDeque<u8>,
        operations: Arc<Mutex<Vec<u8>>>,
        brightness: u8,
        fail_display: bool,
    }
    impl Transport for DeviceDouble {
        fn read(&mut self, bytes: &mut [u8], _: Duration) -> io::Result<usize> {
            if self.input.is_empty() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let count = bytes.len().min(self.input.len());
            for byte in &mut bytes[..count] {
                *byte = self.input.pop_front().unwrap();
            }
            Ok(count)
        }
        fn write(&mut self, bytes: &[u8], _: Duration) -> io::Result<usize> {
            for packet in self.decoder.feed(bytes).unwrap() {
                self.operations.lock().unwrap().push(packet.opcode());
                match packet.opcode() {
                    0x74 => self.brightness = packet.payload()[0],
                    0x44 if self.fail_display => {
                        return Err(io::ErrorKind::ConnectionAborted.into())
                    }
                    0x46 => {
                        let mut payload = vec![0x46, 0x55];
                        let mut data = vec![0; 20];
                        data[0] = 5;
                        data[10] = self.brightness;
                        payload.extend(data);
                        self.input.extend(
                            protocol::encode(
                                &Packet::new(4, payload).unwrap(),
                                ClassicDialect::PixooMax,
                            )
                            .unwrap(),
                        );
                    }
                    _ => {}
                }
            }
            Ok(bytes.len())
        }
    }
    fn session(fail_display: bool) -> (Session<DeviceDouble>, Arc<Mutex<Vec<u8>>>) {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let transport = DeviceDouble {
            decoder: Decoder::new(ClassicDialect::PixooMax),
            input: VecDeque::new(),
            operations: operations.clone(),
            brightness: 100,
            fail_display,
        };
        let session = Session::new(
            transport,
            DeviceProfile::pixoo_max(),
            SessionOptions {
                pacing: Some(Duration::ZERO),
                ..Default::default()
            },
        )
        .unwrap();
        (session, operations)
    }
    fn primed() -> Reply {
        let mut data = vec![0; 20];
        data[0] = 5;
        data[10] = 100;
        Reply {
            opcode: 0x46,
            status: 0x55,
            data,
        }
    }

    #[test]
    fn composed_update_uses_one_session_and_reports_brightness_and_barrier() {
        let (mut session, operations) = session(false);
        let prepared = Prepared {
            action: Action::display(Asset::blank().unwrap()).unwrap(),
            brightness: Some(35),
        };
        let result = execute_session(
            &mut session,
            prepared,
            primed(),
            Instant::now() + Duration::from_secs(1),
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(*operations.lock().unwrap(), vec![0x74, 0x46, 0x44, 0x46]);
        assert_eq!(result["completed_steps"][0]["requested"], 35);
        assert_eq!(result["completed_steps"][0]["observed"], 35);
        assert_eq!(result["completion"], "communication_barrier");
    }

    #[test]
    fn display_failure_preserves_completed_brightness_and_never_replays() {
        let (mut session, operations) = session(true);
        let prepared = Prepared {
            action: Action::display(Asset::blank().unwrap()).unwrap(),
            brightness: Some(35),
        };
        let error = execute_session(
            &mut session,
            prepared,
            primed(),
            Instant::now() + Duration::from_secs(1),
            &CancellationToken::default(),
        )
        .unwrap_err();
        assert_eq!(*operations.lock().unwrap(), vec![0x74, 0x46, 0x44]);
        assert!(error.uncertain);
        assert_eq!(error.completed[0]["requested"], 35);
        assert_eq!(error.completed[0]["observed"], 35);
        assert!(session.is_poisoned());
    }
}
