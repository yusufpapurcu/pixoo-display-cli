# BLE byte transport

The Rust crate has an optional `ble` feature backed by `btleplug`. It carries raw GATT notification and write bytes. BLE envelopes, command encoding, acknowledgement matching, and retries belong to the session or protocol layer. Connecting does not send a device initialization command or change the device clock.

```sh
cargo check --all-features
cargo test --all-features transport::ble::tests
```

## Discovery and selection

`BleDiscovery::scan(adapter_index, scan_for, timeout)` scans one adapter and returns its known peripherals. The timeout includes adapter enumeration, the scan, stopping the scan, and reading peripheral properties. `scan_for` must be shorter than `timeout`. Results can include previously cached peripherals. Scan cancellation schedules a bounded attempt to stop scanning in the background.

`devices()` returns the platform peripheral ID, Bluetooth address, advertising name, and advertised services. A name is descriptive only. `connect` consumes the discovery and accepts an exact `BleSelector::Id` or `BleSelector::Address`. Missing or ambiguous selections fail. macOS peripheral IDs are usable when Bluetooth addresses are unavailable.

```rust,no_run
use divoom_bluetooth::transport::{Transport, ble::{BleDiscovery, BleOptions, BleSelector}};
use std::{io, time::Duration};

fn connect_selected(id: String) -> io::Result<()> {
    let discovery = BleDiscovery::scan(0, Duration::from_secs(3), Duration::from_secs(6))?;
    for device in discovery.devices() {
        println!("{} {} {:?}", device.id, device.address, device.local_name);
    }
    let mut transport = discovery.connect(
        BleSelector::Id(id),
        BleOptions::default(),
        Duration::from_secs(8),
    )?;
    // Pass transport to a session configured for the selected device's protocol.
    transport.close()?;
    Ok(())
}
```

`connect` discovers GATT services and requires the configured characteristics in the configured service. It verifies write and notification properties. It never selects a characteristic based on a device name.

## Characteristic and buffer options

The default options specify one commonly used Divoom GATT layout. These UUIDs do not establish compatibility with a particular device model or firmware.

| Option | Default |
| --- | --- |
| `service_uuid` | `49535343-fe7d-4ae5-8fa9-9fafd205e455` |
| `write_uuid` | `49535343-8841-43f4-a8d4-ecbe34729bb3` |
| `notify_uuid` | `49535343-1e4d-4bd9-ba61-23c647249616` |
| `write_type` | `WriteType::WithoutResponse` |
| `max_write_len` | 20 bytes |
| `notification_queue_capacity` | 64 notifications |
| `max_notification_len` | 512 bytes |
| `subscription` | `Subscription::Owned` |
| `close_timeout` | 2 seconds |

Different device families can override every UUID and select `WriteType::WithResponse`. `max_write_len` accepts 1 through 512 bytes. Choose a payload size supported by the negotiated ATT MTU and device. `btleplug` has no portable MTU query, so the adapter does not infer a larger payload size.

The notification queue accepts 1 through 65,536 packets. Each packet has a configured limit of 1 through 512 bytes. The adapter retains at most one additional partial notification outside the queue. Notifications for other characteristic UUIDs and empty notifications are ignored.

## Reads, writes, and deadlines

`Transport::read` returns notification bytes and retains the unconsumed tail for later reads. Notification boundaries have no protocol meaning. A read deadline returns `TimedOut`. A zero timeout polls already queued data and returns `WouldBlock` if none is ready. An ended notification stream returns EOF after buffered bytes. A disconnected peripheral can leave the backend stream open, in which case the read reaches its deadline.

`Transport::write` sends at most `max_write_len` bytes per call and returns the completed chunk length. Callers must handle short writes. The session can continue with the remaining bytes under its transaction deadline. A zero timeout returns `WouldBlock` without issuing a GATT write.

Any error from an attempted GATT write, including a timeout, returns `ConnectionAborted` and closes the transport for further reads and writes. A platform operation may complete after its future is cancelled, so delivery is uncertain. Reusing the stream or automatically retrying that chunk can duplicate bytes. `close()` still releases resources after this error.

The adapter's queue overflow and oversized notification errors are sticky `InvalidData` failures. Reads and writes fail until the transport is reopened. Overflow stops the notification pump and is never silently ignored. The Windows and macOS `btleplug` 0.11.8 implementations discard their own broadcast lag errors before exposing the notification stream. The adapter cannot detect that upstream loss or loss in the operating system. Protocol acknowledgements remain necessary.

Each operation wraps its async work in a Tokio timeout. Like other userspace timeouts, these bounds depend on executor scheduling and on the backend yielding. They do not impose a hard real time deadline on operating system Bluetooth work.

## Runtime and ownership

The synchronous adapter owns a multi-thread Tokio runtime. Synchronous construction, reads, writes, and close must run outside a Tokio runtime thread. They return `WouldBlock` when called inside Tokio. Async applications can move these operations and the transport into `tokio::task::spawn_blocking` or a dedicated blocking thread. Do not call the synchronous adapter from a Tokio task and expect nested `block_on` to work.

`BleTransport::from_connected(peripheral, runtime, options, timeout)` consumes a caller-provided multi-thread runtime and borrows an already connected peripheral. That runtime must have its timer enabled, for example through `Builder::new_multi_thread().enable_all()`. The caller must give the adapter exclusive use of the runtime. This constructor never calls `connect` and never disconnects the borrowed peripheral, even on setup failure.

When discovery connects a peripheral that was disconnected, the transport owns that connection attempt and disconnects it during close. A connection that was already open remains borrowed. Ownership assumes no competing process or handle changes the same connection during setup. `btleplug` exposes connection state, but no atomic connection lease across applications.

`Subscription::Owned` subscribes during setup and releases the subscription during close. Disconnecting an owned link also removes its subscription. `Subscription::Existing` uses a subscription established by the caller and never unsubscribes. `btleplug` cannot report whether a characteristic was already subscribed, so callers borrowing a link must choose this option deliberately when they do not own the subscription.

`close()` attempts cleanup within `close_timeout` and reports failure. Dropping the adapter schedules a bounded cleanup attempt on a separate thread. Setup failure does the same for a connection or subscription it attempted to open. Background cleanup is best effort. A process exit, a stuck backend, or a late platform connection can prevent release.

## Platform evidence

The implementation uses `btleplug` platform backends for Windows, Linux, and macOS. Windows requires Bluetooth availability and the relevant operating system permissions. Linux uses BlueZ and requires D-Bus development libraries when compiling its backend. macOS requires Bluetooth permission and an application environment that supplies the appropriate Bluetooth usage description.

The Windows feature build and hardware-free notification tests are checked in this workspace. macOS BLE also cross-compiles from this Windows host. Linux BLE compilation and BLE hardware behavior on every platform remain unverified. No device compatibility, successful pairing, GATT delivery, or device reply behavior is established by the unit tests or a successful compiler run.
