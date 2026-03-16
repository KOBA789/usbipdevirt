// Protocol version
pub const USBIP_VERSION: u16 = 0x0111;

// Operation codes
pub const OP_REQ_DEVLIST: u16 = 0x8005;
pub const OP_REP_DEVLIST: u16 = 0x0005;
pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const OP_REP_IMPORT: u16 = 0x0003;

// URB commands
pub const USBIP_CMD_SUBMIT: u32 = 1;
pub const USBIP_RET_SUBMIT: u32 = 3;
pub const USBIP_CMD_UNLINK: u32 = 2;
pub const USBIP_RET_UNLINK: u32 = 4;

// Field sizes
pub const PATH_LEN: usize = 256;
pub const BUSID_LEN: usize = 32;

/// USB device information from a USB/IP server.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: String,
    pub busid: String,
    pub busnum: u32,
    pub devnum: u32,
    pub speed: u32,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub device_class: u8,
    pub device_sub_class: u8,
    pub device_protocol: u8,
    pub configuration_value: u8,
    pub num_configurations: u8,
    pub num_interfaces: u8,
    pub interfaces: Vec<InterfaceInfo>,
}

/// USB interface information.
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
}

/// USB transfer direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out = 0,
    In = 1,
}

/// Response to a submitted URB.
#[derive(Debug)]
pub struct RetSubmit {
    pub seqnum: u32,
    pub status: i32,
    pub actual_length: u32,
    pub data: Vec<u8>,
    pub direction: Direction,
}

/// Response to an unlink request.
#[derive(Debug)]
pub struct RetUnlink {
    pub seqnum: u32,
    pub status: i32,
}

/// A response received from the USB/IP server.
#[derive(Debug)]
pub enum UrbResponse {
    Submit(RetSubmit),
    Unlink(RetUnlink),
}

pub(crate) fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
