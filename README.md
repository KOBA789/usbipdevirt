# usbipdevirt

USB/IP client that materializes remote USB devices as local physical USB devices via Linux [raw-gadget](https://github.com/xairy/raw-gadget).

Typical USB/IP makes local devices available remotely. This tool does the reverse — it imports a remote USB device and re-creates it locally using a USB Device Controller (UDC), so the host sees it as a real physically-attached device.

## Requirements

- Linux with raw-gadget kernel module loaded (`/dev/raw-gadget`)
- A USB Device Controller (e.g., `dwc2` on Raspberry Pi)
- A USB/IP server exposing devices

## Usage

```
usbipdevirt [--host <HOST>] [--port <PORT>] [--udc-driver <DRIVER>] [--udc-device <DEVICE>]
```

| Option         | Default            | Description        |
|----------------|--------------------|--------------------|
| `--host`       | `localhost`        | USB/IP server host |
| `--port`       | `3240`             | USB/IP server port |
| `--udc-driver` | `1000480000.usb`   | UDC driver name    |
| `--udc-device` | `1000480000.usb`   | UDC device name    |

Root or appropriate device permissions required.

## Build

```
cargo build
```
