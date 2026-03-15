use std::mem::size_of;

pub const UDC_NAME_LENGTH_MAX: usize = 128;
pub const USB_RAW_EPS_NUM_MAX: usize = 30;
pub const USB_RAW_EP_NAME_MAX: usize = 16;
pub const USB_RAW_EP_ADDR_ANY: u32 = 0xff;

pub const USB_RAW_IO_FLAGS_ZERO: u16 = 0x0001;

/// Argument for `USB_RAW_IOCTL_INIT`.
#[repr(C)]
pub struct UsbRawInit {
    pub driver_name: [u8; UDC_NAME_LENGTH_MAX],
    pub device_name: [u8; UDC_NAME_LENGTH_MAX],
    pub speed: u8,
}

const _: () = assert!(size_of::<UsbRawInit>() == 257);

/// Header for `USB_RAW_IOCTL_EVENT_FETCH` (flexible array member).
#[repr(C)]
pub(crate) struct UsbRawEventHeader {
    pub event_type: u32,
    pub length: u32,
}

const _: () = assert!(size_of::<UsbRawEventHeader>() == 8);

/// Header for `USB_RAW_IOCTL_EP0/EP_WRITE/READ` (flexible array member).
#[repr(C)]
pub(crate) struct UsbRawEpIoHeader {
    pub ep: u16,
    pub flags: u16,
    pub length: u32,
}

const _: () = assert!(size_of::<UsbRawEpIoHeader>() == 8);

/// Endpoint capabilities bitfield.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct UsbRawEpCaps {
    bits: u32,
}

impl UsbRawEpCaps {
    pub fn type_control(&self) -> bool {
        self.bits & (1 << 0) != 0
    }
    pub fn type_iso(&self) -> bool {
        self.bits & (1 << 1) != 0
    }
    pub fn type_bulk(&self) -> bool {
        self.bits & (1 << 2) != 0
    }
    pub fn type_int(&self) -> bool {
        self.bits & (1 << 3) != 0
    }
    pub fn dir_in(&self) -> bool {
        self.bits & (1 << 4) != 0
    }
    pub fn dir_out(&self) -> bool {
        self.bits & (1 << 5) != 0
    }
}

const _: () = assert!(size_of::<UsbRawEpCaps>() == 4);

/// Endpoint limits.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct UsbRawEpLimits {
    pub maxpacket_limit: u16,
    pub max_streams: u16,
    pub reserved: u32,
}

const _: () = assert!(size_of::<UsbRawEpLimits>() == 8);

/// Information about a gadget endpoint.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct UsbRawEpInfo {
    pub name: [u8; USB_RAW_EP_NAME_MAX],
    pub addr: u32,
    pub caps: UsbRawEpCaps,
    pub limits: UsbRawEpLimits,
}

const _: () = assert!(size_of::<UsbRawEpInfo>() == 32);

/// Argument for `USB_RAW_IOCTL_EPS_INFO`.
#[repr(C)]
pub struct UsbRawEpsInfo {
    pub eps: [UsbRawEpInfo; USB_RAW_EPS_NUM_MAX],
}

impl Default for UsbRawEpsInfo {
    fn default() -> Self {
        Self {
            eps: [UsbRawEpInfo::default(); USB_RAW_EPS_NUM_MAX],
        }
    }
}

const _: () = assert!(size_of::<UsbRawEpsInfo>() == 960);
