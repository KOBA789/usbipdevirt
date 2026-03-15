use std::io::{self, Read, Write};

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

// Serialization helpers

pub(crate) fn read_u8(r: &mut impl Read) -> io::Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

pub(crate) fn read_u16_be(r: &mut impl Read) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_be_bytes(buf))
}

pub(crate) fn read_u32_be(r: &mut impl Read) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

pub(crate) fn read_exact_array<const N: usize>(r: &mut impl Read) -> io::Result<[u8; N]> {
    let mut buf = [0u8; N];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

pub(crate) fn write_u16_be(w: &mut impl Write, v: u16) -> io::Result<()> {
    w.write_all(&v.to_be_bytes())
}

pub(crate) fn write_u32_be(w: &mut impl Write, v: u32) -> io::Result<()> {
    w.write_all(&v.to_be_bytes())
}

pub(crate) fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

pub(crate) fn read_device_info(r: &mut impl Read) -> io::Result<DeviceInfo> {
    let path_bytes: [u8; PATH_LEN] = read_exact_array(r)?;
    let busid_bytes: [u8; BUSID_LEN] = read_exact_array(r)?;
    let busnum = read_u32_be(r)?;
    let devnum = read_u32_be(r)?;
    let speed = read_u32_be(r)?;
    let id_vendor = read_u16_be(r)?;
    let id_product = read_u16_be(r)?;
    let bcd_device = read_u16_be(r)?;
    let device_class = read_u8(r)?;
    let device_sub_class = read_u8(r)?;
    let device_protocol = read_u8(r)?;
    let configuration_value = read_u8(r)?;
    let num_configurations = read_u8(r)?;
    let num_interfaces = read_u8(r)?;

    Ok(DeviceInfo {
        path: bytes_to_string(&path_bytes),
        busid: bytes_to_string(&busid_bytes),
        busnum,
        devnum,
        speed,
        id_vendor,
        id_product,
        bcd_device,
        device_class,
        device_sub_class,
        device_protocol,
        configuration_value,
        num_configurations,
        num_interfaces,
        interfaces: Vec::new(),
    })
}

pub(crate) fn read_interface_info(r: &mut impl Read) -> io::Result<InterfaceInfo> {
    let class = read_u8(r)?;
    let sub_class = read_u8(r)?;
    let protocol = read_u8(r)?;
    let _padding = read_u8(r)?;
    Ok(InterfaceInfo {
        class,
        sub_class,
        protocol,
    })
}
