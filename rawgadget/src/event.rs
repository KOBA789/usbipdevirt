use crate::usb_types::UsbCtrlRequest;

/// An event fetched from the raw-gadget device.
#[derive(Debug)]
pub enum Event {
    Connect,
    Control(UsbCtrlRequest),
    Suspend,
    Resume,
    Reset,
    Disconnect,
    Unknown(u32),
}
