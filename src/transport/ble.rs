//! Optional GATT byte transport. See `docs/ble.md` for platform and runtime limits.
use super::{BluetoothAddress, Transport};
pub use btleplug::api::WriteType;
use btleplug::{
    api::{
        Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter,
        ValueNotification,
    },
    platform::{Manager, Peripheral},
};
use futures_util::{Stream, StreamExt};
use std::{
    collections::VecDeque,
    future::Future,
    io,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    runtime::{Builder, Handle, Runtime, RuntimeFlavor},
    sync::mpsc,
    task::JoinHandle,
};
pub use uuid::Uuid;

pub const DIVOOM_SERVICE: Uuid = Uuid::from_u128(0x49535343_fe7d_4ae5_8fa9_9fafd205e455);
pub const DIVOOM_WRITE: Uuid = Uuid::from_u128(0x49535343_8841_43f4_a8d4_ecbe34729bb3);
pub const DIVOOM_NOTIFY: Uuid = Uuid::from_u128(0x49535343_1e4d_4bd9_ba61_23c647249616);

/// Exact identity selection. Advertising names are descriptive only.
#[derive(Clone, Debug)]
pub enum BleSelector {
    Id(String),
    Address(BluetoothAddress),
}

#[derive(Clone, Debug)]
pub struct BleDevice {
    pub id: String,
    /// macOS uses peripheral IDs and may not expose a usable Bluetooth address.
    pub address: String,
    pub local_name: Option<String>,
    pub services: Vec<Uuid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subscription {
    /// Subscribe during setup and unsubscribe during close.
    Owned,
    /// The caller already subscribed. Never unsubscribe during close.
    Existing,
}

#[derive(Clone, Debug)]
pub struct BleOptions {
    pub service_uuid: Uuid,
    pub write_uuid: Uuid,
    pub notify_uuid: Uuid,
    pub write_type: WriteType,
    /// ATT payload size, chosen by the caller. btleplug has no portable MTU query.
    pub max_write_len: usize,
    pub notification_queue_capacity: usize,
    pub max_notification_len: usize,
    pub subscription: Subscription,
    pub close_timeout: Duration,
}

impl Default for BleOptions {
    fn default() -> Self {
        Self {
            service_uuid: DIVOOM_SERVICE,
            write_uuid: DIVOOM_WRITE,
            notify_uuid: DIVOOM_NOTIFY,
            write_type: WriteType::WithoutResponse,
            max_write_len: 20,
            notification_queue_capacity: 64,
            max_notification_len: 512,
            subscription: Subscription::Owned,
            close_timeout: Duration::from_secs(2),
        }
    }
}

impl BleOptions {
    fn validate(&self) -> io::Result<()> {
        if self.max_write_len == 0 || self.max_write_len > 512 {
            return Err(invalid("max_write_len must be 1..=512"));
        }
        if !(1..=65_536).contains(&self.notification_queue_capacity)
            || !(1..=512).contains(&self.max_notification_len)
        {
            return Err(invalid(
                "notification queue capacity must be 1..=65536 and length must be 1..=512",
            ));
        }
        Ok(())
    }
}

// shutdown_background also permits destruction from an async context.
struct OwnedRuntime(Option<Runtime>);
impl OwnedRuntime {
    fn get(&self) -> &Runtime {
        self.0.as_ref().expect("live BLE runtime")
    }
    fn take(&mut self) -> Runtime {
        self.0.take().expect("live BLE runtime")
    }
}
impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

/// Scan results retain the runtime and platform peripheral handles until selection.
pub struct BleDiscovery {
    runtime: OwnedRuntime,
    devices: Vec<(BleDevice, Peripheral)>,
}

impl BleDiscovery {
    /// Scan the adapter at `adapter_index`. `timeout` covers the whole operation.
    pub fn scan(adapter_index: usize, scan_for: Duration, timeout: Duration) -> io::Result<Self> {
        ensure_sync()?;
        if scan_for >= timeout {
            return Err(invalid("scan_for must be less than timeout"));
        }
        let deadline = deadline(timeout)?;
        let mut runtime = OwnedRuntime(Some(
            Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        ));
        let adapter = run(runtime.get(), deadline, async {
            let manager = Manager::new().await.map_err(backend)?;
            manager
                .adapters()
                .await
                .map_err(backend)?
                .into_iter()
                .nth(adapter_index)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "BLE adapter index not found")
                })
        })?;
        let result = run(runtime.get(), deadline, async {
            adapter
                .start_scan(ScanFilter::default())
                .await
                .map_err(backend)?;
            tokio::time::sleep(scan_for).await;
            adapter.stop_scan().await.map_err(backend)?;
            let mut devices = Vec::new();
            for peripheral in adapter.peripherals().await.map_err(backend)? {
                let properties = peripheral.properties().await.map_err(backend)?;
                devices.push((
                    BleDevice {
                        id: peripheral.id().to_string(),
                        address: peripheral.address().to_string(),
                        local_name: properties.as_ref().and_then(|p| p.local_name.clone()),
                        services: properties.map(|p| p.services).unwrap_or_default(),
                    },
                    peripheral,
                ));
            }
            Ok(devices)
        });
        match result {
            Ok(devices) => Ok(Self { runtime, devices }),
            Err(error) => {
                background_cleanup(runtime.take(), Duration::from_secs(2), async move {
                    adapter.stop_scan().await.map_err(backend)
                });
                Err(error)
            }
        }
    }

    pub fn devices(&self) -> impl Iterator<Item = &BleDevice> {
        self.devices.iter().map(|(info, _)| info)
    }

    /// Consume the discovery and connect only the selected ID or address.
    pub fn connect(
        mut self,
        selector: BleSelector,
        options: BleOptions,
        timeout: Duration,
    ) -> io::Result<BleTransport> {
        ensure_sync()?;
        options.validate()?;
        let matches: Vec<_> = self
            .devices
            .iter()
            .filter(|(info, _)| match &selector {
                BleSelector::Id(id) => &info.id == id,
                BleSelector::Address(address) => {
                    info.address.eq_ignore_ascii_case(&address.to_string())
                }
            })
            .collect();
        if matches.len() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "BLE selector must match exactly one discovered peripheral",
            ));
        }
        let peripheral = matches[0].1.clone();
        BleTransport::open(
            peripheral,
            OwnedRuntime(Some(self.runtime.take())),
            options,
            timeout,
            true,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionLease {
    Borrowed,
    Owned,
}

struct NotificationInbox {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: VecDeque<u8>,
    fault: Arc<AtomicU8>,
}

impl NotificationInbox {
    fn check(&self) -> io::Result<()> {
        match self.fault.load(Ordering::Acquire) {
            0 => Ok(()),
            1 => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "BLE notification queue overflow; reopen the transport",
            )),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "BLE notification exceeds configured length; reopen the transport",
            )),
        }
    }

    fn copy_pending(&mut self, bytes: &mut [u8]) -> usize {
        let count = bytes.len().min(self.pending.len());
        for dest in &mut bytes[..count] {
            *dest = self.pending.pop_front().expect("pending length checked");
        }
        count
    }

    async fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if self.pending.is_empty() {
            match self.rx.recv().await {
                Some(packet) => self.pending.extend(packet),
                None => {
                    self.check()?;
                    return Ok(0);
                }
            }
        }
        self.check()?;
        Ok(self.copy_pending(bytes))
    }

    fn try_read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if self.pending.is_empty() {
            match self.rx.try_recv() {
                Ok(packet) => self.pending.extend(packet),
                Err(mpsc::error::TryRecvError::Empty) => {
                    return Err(io::ErrorKind::WouldBlock.into())
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.check()?;
                    return Ok(0);
                }
            }
        }
        self.check()?;
        Ok(self.copy_pending(bytes))
    }
}

/// Synchronous byte adapter with an owned multi-thread Tokio runtime.
/// Call synchronous operations outside Tokio, or use `spawn_blocking`.
pub struct BleTransport {
    runtime: OwnedRuntime,
    peripheral: Peripheral,
    write_characteristic: Characteristic,
    notify_characteristic: Characteristic,
    options: BleOptions,
    lease: ConnectionLease,
    inbox: NotificationInbox,
    pump: JoinHandle<()>,
    closed: bool,
}

impl BleTransport {
    /// Borrow an already connected peripheral, consuming an exclusive runtime.
    /// This constructor never disconnects the borrowed peripheral, including on error.
    pub fn from_connected(
        peripheral: Peripheral,
        runtime: Runtime,
        options: BleOptions,
        timeout: Duration,
    ) -> io::Result<Self> {
        Self::open(
            peripheral,
            OwnedRuntime(Some(runtime)),
            options,
            timeout,
            false,
        )
    }

    fn open(
        peripheral: Peripheral,
        mut runtime: OwnedRuntime,
        options: BleOptions,
        timeout: Duration,
        allow_connect: bool,
    ) -> io::Result<Self> {
        ensure_sync()?;
        options.validate()?;
        if runtime.get().handle().runtime_flavor() != RuntimeFlavor::MultiThread {
            return Err(invalid("BLE needs a multi-thread Tokio runtime"));
        }
        let deadline = deadline(timeout)?;
        let mut lease = ConnectionLease::Borrowed;
        let mut subscription_attempted = false;
        let mut notify_for_cleanup = None;
        let setup = run(runtime.get(), deadline, async {
            if !peripheral.is_connected().await.map_err(backend)? {
                if !allow_connect {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "BLE peripheral is not connected",
                    ));
                }
                // A timed-out connect can still complete in the platform backend.
                lease = ConnectionLease::Owned;
                peripheral.connect().await.map_err(backend)?;
            }
            peripheral.discover_services().await.map_err(backend)?;
            let characteristics = peripheral.characteristics();
            let find = |uuid| {
                characteristics
                    .iter()
                    .find(|c| c.service_uuid == options.service_uuid && c.uuid == uuid)
                    .cloned()
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::NotFound,
                            "configured BLE characteristic not found in configured service",
                        )
                    })
            };
            let write = find(options.write_uuid)?;
            let notify = find(options.notify_uuid)?;
            let write_flag = match options.write_type {
                WriteType::WithResponse => CharPropFlags::WRITE,
                WriteType::WithoutResponse => CharPropFlags::WRITE_WITHOUT_RESPONSE,
            };
            if !write.properties.contains(write_flag) {
                return Err(invalid(
                    "configured BLE write type is unsupported by characteristic",
                ));
            }
            if !notify
                .properties
                .intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
            {
                return Err(invalid(
                    "configured BLE characteristic cannot notify or indicate",
                ));
            }
            let stream = peripheral.notifications().await.map_err(backend)?;
            if options.subscription == Subscription::Owned {
                notify_for_cleanup = Some(notify.clone());
                subscription_attempted = true;
                peripheral.subscribe(&notify).await.map_err(backend)?;
            }
            Ok((write, notify, stream))
        });
        let (write_characteristic, notify_characteristic, stream) = match setup {
            Ok(setup) => setup,
            Err(error) => {
                let cleanup_peripheral = peripheral.clone();
                let notify = if subscription_attempted {
                    notify_for_cleanup
                } else {
                    None
                };
                background_cleanup(runtime.take(), options.close_timeout, async move {
                    cleanup_link(&cleanup_peripheral, notify.as_ref(), lease).await
                });
                return Err(error);
            }
        };
        let (tx, rx) = mpsc::channel(options.notification_queue_capacity);
        let fault = Arc::new(AtomicU8::new(0));
        let pump_fault = fault.clone();
        let max_len = options.max_notification_len;
        let notify_uuid = options.notify_uuid;
        let pump = runtime.get().spawn(pump_notifications(
            stream,
            tx,
            pump_fault,
            notify_uuid,
            max_len,
        ));
        Ok(Self {
            runtime,
            peripheral,
            write_characteristic,
            notify_characteristic,
            options,
            lease,
            inbox: NotificationInbox {
                rx,
                pending: VecDeque::new(),
                fault,
            },
            pump,
            closed: false,
        })
    }
}

impl Transport for BleTransport {
    fn read(&mut self, bytes: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.closed {
            return Err(io::ErrorKind::NotConnected.into());
        }
        ensure_sync()?;
        if timeout.is_zero() {
            return self.inbox.try_read(bytes);
        }
        run(
            self.runtime.get(),
            deadline(timeout)?,
            self.inbox.read(bytes),
        )
    }

    fn write(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.closed {
            return Err(io::ErrorKind::NotConnected.into());
        }
        ensure_sync()?;
        self.inbox.check()?;
        // GATT writes have no portable nonblocking completion query.
        if timeout.is_zero() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = bytes.len().min(self.options.max_write_len);
        let result = run(self.runtime.get(), deadline(timeout)?, async {
            self.peripheral
                .write(
                    &self.write_characteristic,
                    &bytes[..count],
                    self.options.write_type,
                )
                .await
                .map_err(backend)
        });
        match result {
            Ok(()) => Ok(count),
            Err(error) => {
                // A backend may complete a cancelled GATT write. Retrying this stream is unsafe.
                self.closed = true;
                Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    format!("BLE write delivery is uncertain; reopen the transport: {error}"),
                ))
            }
        }
    }

    fn close(&mut self) -> io::Result<()> {
        ensure_sync()?;
        self.pump.abort();
        self.closed = true;
        let notify = (self.options.subscription == Subscription::Owned)
            .then_some(&self.notify_characteristic);
        let result = run(
            self.runtime.get(),
            deadline(self.options.close_timeout)?,
            cleanup_link(&self.peripheral, notify, self.lease),
        );
        if result.is_ok() {
            self.lease = ConnectionLease::Borrowed;
            self.options.subscription = Subscription::Existing;
        }
        result
    }
}

impl Drop for BleTransport {
    fn drop(&mut self) {
        self.pump.abort();
        let peripheral = self.peripheral.clone();
        let notify = (self.options.subscription == Subscription::Owned)
            .then(|| self.notify_characteristic.clone());
        let lease = self.lease;
        if notify.is_some() || lease == ConnectionLease::Owned {
            background_cleanup(
                self.runtime.take(),
                self.options.close_timeout,
                async move { cleanup_link(&peripheral, notify.as_ref(), lease).await },
            );
        }
    }
}

async fn cleanup_link(
    peripheral: &Peripheral,
    notify: Option<&Characteristic>,
    lease: ConnectionLease,
) -> io::Result<()> {
    // Disconnect removes subscriptions too and must not wait behind unsubscribe.
    if lease == ConnectionLease::Owned {
        return peripheral.disconnect().await.map_err(backend);
    }
    if let Some(notify) = notify {
        return peripheral.unsubscribe(notify).await.map_err(backend);
    }
    Ok(())
}

async fn pump_notifications(
    mut stream: impl Stream<Item = ValueNotification> + Unpin,
    tx: mpsc::Sender<Vec<u8>>,
    fault: Arc<AtomicU8>,
    uuid: Uuid,
    max_len: usize,
) {
    while let Some(notification) = stream.next().await {
        if notification.uuid != uuid || notification.value.is_empty() {
            continue;
        }
        if notification.value.len() > max_len {
            fault.store(2, Ordering::Release);
            break;
        }
        if let Err(error) = tx.try_send(notification.value) {
            if matches!(error, mpsc::error::TrySendError::Full(_)) {
                fault.store(1, Ordering::Release);
            }
            break;
        }
    }
}

fn background_cleanup<F>(runtime: Runtime, timeout: Duration, cleanup: F)
where
    F: Future<Output = io::Result<()>> + Send + 'static,
{
    // Keep cleanup alive beyond the caller's deadline without blocking an async drop.
    let runtime = OwnedRuntime(Some(runtime));
    let _ = std::thread::Builder::new()
        .name("divoom-ble-cleanup".into())
        .spawn(move || {
            let _ = run(
                runtime.get(),
                Instant::now()
                    .checked_add(timeout)
                    .unwrap_or_else(Instant::now),
                cleanup,
            );
        });
}

fn ensure_sync() -> io::Result<()> {
    if Handle::try_current().is_ok() {
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "synchronous BLE operations cannot run inside Tokio; use spawn_blocking",
        ))
    } else {
        Ok(())
    }
}
fn deadline(timeout: Duration) -> io::Result<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| invalid("BLE timeout is too large"))
}
fn run<T>(
    runtime: &Runtime,
    deadline: Instant,
    future: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    runtime.block_on(async {
        tokio::time::timeout(remaining, future)
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    })
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn backend(error: btleplug::Error) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbox(capacity: usize) -> (mpsc::Sender<Vec<u8>>, NotificationInbox) {
        let (tx, rx) = mpsc::channel(capacity);
        (
            tx,
            NotificationInbox {
                rx,
                pending: VecDeque::new(),
                fault: Arc::new(AtomicU8::new(0)),
            },
        )
    }

    #[test]
    fn partial_reads_retain_notification_tail_and_zero_poll_does_not_consume() {
        let (tx, mut inbox) = inbox(2);
        let mut bytes = [0; 2];
        assert_eq!(
            inbox.try_read(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        tx.try_send(vec![1, 2, 3]).unwrap();
        tx.try_send(vec![4]).unwrap();
        assert_eq!(inbox.try_read(&mut bytes).unwrap(), 2);
        assert_eq!(bytes, [1, 2]);
        assert_eq!(inbox.try_read(&mut bytes).unwrap(), 1);
        assert_eq!(bytes[0], 3);
        assert_eq!(inbox.try_read(&mut bytes).unwrap(), 1);
        assert_eq!(bytes[0], 4);
    }

    #[test]
    fn overflow_is_sticky_and_blocks_queued_or_partial_data() {
        let (tx, mut inbox) = inbox(1);
        tx.try_send(vec![1, 2]).unwrap();
        let mut bytes = [0; 1];
        assert_eq!(inbox.try_read(&mut bytes).unwrap(), 1);
        inbox.fault.store(1, Ordering::Release);
        for _ in 0..2 {
            assert_eq!(
                inbox.try_read(&mut bytes).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn notification_pump_reports_overflow_and_oversized_packets() {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        for (capacity, max_len, expected_fault) in [(1, 2, 1), (2, 1, 2)] {
            let (tx, mut inbox) = inbox(capacity);
            let packets = futures_util::stream::iter([
                ValueNotification {
                    uuid: DIVOOM_WRITE,
                    value: vec![9],
                },
                ValueNotification {
                    uuid: DIVOOM_NOTIFY,
                    value: vec![],
                },
                ValueNotification {
                    uuid: DIVOOM_NOTIFY,
                    value: vec![1, 2],
                },
                ValueNotification {
                    uuid: DIVOOM_NOTIFY,
                    value: vec![3],
                },
            ]);
            runtime.block_on(pump_notifications(
                packets,
                tx,
                inbox.fault.clone(),
                DIVOOM_NOTIFY,
                max_len,
            ));
            assert_eq!(inbox.fault.load(Ordering::Acquire), expected_fault);
            assert_eq!(
                inbox.try_read(&mut [0; 1]).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn sync_adapter_rejects_nested_tokio_without_panicking() {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async { ensure_sync() });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn waiting_for_notifications_has_a_deadline() {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (_tx, mut inbox) = inbox(1);
        let mut bytes = [0; 8];
        let result = run(
            &runtime,
            deadline(Duration::from_millis(5)).unwrap(),
            inbox.read(&mut bytes),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn closed_notification_stream_is_eof() {
        let (tx, mut inbox) = inbox(1);
        drop(tx);
        assert_eq!(inbox.try_read(&mut [0; 1]).unwrap(), 0);
    }
}
