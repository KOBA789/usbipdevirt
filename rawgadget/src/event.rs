use crate::usb_types::UsbCtrlRequest;

// raw-gadget event types (from raw_gadget.h)
pub(crate) const USB_RAW_EVENT_CONNECT: u32 = 1;
pub(crate) const USB_RAW_EVENT_CONTROL: u32 = 2;
pub(crate) const USB_RAW_EVENT_SUSPEND: u32 = 3;
pub(crate) const USB_RAW_EVENT_RESUME: u32 = 4;
pub(crate) const USB_RAW_EVENT_RESET: u32 = 5;
pub(crate) const USB_RAW_EVENT_DISCONNECT: u32 = 6;

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
