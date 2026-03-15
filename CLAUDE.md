# usbipreifier

## Project Overview

usbipreifier is a tool that materializes remote USB devices exposed via USB/IP as physical USB devices using Linux's raw-gadget interface. It acts as a bridge: it connects to a USB/IP server as a client, imports a remote USB device, and then re-creates that device locally using raw-gadget so it appears as a real physical USB device.

Think of it as the reverse of typical USB/IP usage — instead of making a local device available remotely, it makes a remote device available locally as if it were physically plugged in.

## Architecture

The project consists of three crates (planned as a Cargo workspace):

1. **rawgadget** — Safe Rust wrapper around the Linux raw-gadget kernel interface (`/dev/raw-gadget`). Handles ioctl calls, event fetching, endpoint management, and data transfers.

2. **usbip** — USB/IP protocol client library. Implements the wire protocol (device listing, device import, URB submission/completion) over TCP.

3. **usbipreifier** — The main application binary that connects the above two crates. It imports a USB device via USB/IP and re-emits it as a physical gadget via raw-gadget.

## Hardware Environment

- **Platform**: Raspberry Pi 5 (aarch64)
- **UDC**: `dwc2` driver, driver_name and device_name are both `1000480000.usb`
- **Setup**: The USB OTG (Device) port is connected via cable back to one of the Pi's own USB Host ports, so gadgets created via raw-gadget are visible to the same machine (verifiable with `lsusb`).
- **Raw-gadget device**: `/dev/raw-gadget`
- **Reference source**: raw-gadget kernel module source is at `../raw-gadget`
- **USB/IP server for debugging**: A USB/IP server runs on `localhost:3240` with an echo-back CDC ACM device attached. Useful for development and testing without needing an external USB/IP host.

## Build & Run

```sh
cargo build
cargo run
```

Interacting with `/dev/raw-gadget` requires root or appropriate device permissions.

## Key Technical Details

### raw-gadget Interface

- Device file: `/dev/raw-gadget`
- Interaction: open fd, then ioctl calls (INIT → RUN → event loop with EVENT_FETCH)
- Key ioctls: INIT, RUN, EVENT_FETCH, EP0_READ, EP0_WRITE, EP_ENABLE, EP_DISABLE, EP_READ, EP_WRITE, EPS_INFO, CONFIGURE, VBUS_DRAW, EP0_STALL, EP_SET_HALT, EP_CLEAR_HALT, EP_SET_WEDGE
- All ioctl calls are blocking
- Typical pattern: main thread handles events, spawns threads per active endpoint
- Header file: `../raw-gadget/raw_gadget/raw_gadget.h`

### USB/IP Protocol

- TCP port 3240, all fields big-endian
- Two phases: device management (list/import) and URB transfer
- Management opcodes: OP_REQ_DEVLIST (0x8005), OP_REP_DEVLIST (0x0005), OP_REQ_IMPORT (0x8003), OP_REP_IMPORT (0x0003)
- URB commands: USBIP_CMD_SUBMIT (0x01), USBIP_RET_SUBMIT (0x03), USBIP_CMD_UNLINK (0x02), USBIP_RET_UNLINK (0x04)
- All URB headers are exactly 48 bytes
- After successful import, the TCP connection stays open for URB transfer

## Conventions

- Rust edition: 2024
- Minimum Rust version: 1.92.0
- Async runtime: TBD (tokio is a likely choice)
- Error handling: TBD (anyhow or thiserror)
