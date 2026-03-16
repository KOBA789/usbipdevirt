use std::io::{self, Read};
use std::mem::size_of;

use zerocopy::byteorder::network_endian::{I32, U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::protocol::{bytes_to_string, DeviceInfo, InterfaceInfo};

// --- I/O helper ---

pub(crate) fn read_wire<T: FromBytes + IntoBytes>(r: &mut impl Read) -> io::Result<T> {
    let mut val = T::new_zeroed();
    r.read_exact(val.as_mut_bytes())?;
    Ok(val)
}

// --- Wire format types ---

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct OpHeader {
    pub version: U16,
    pub code: U16,
    pub status: U32,
}
const _: () = assert!(size_of::<OpHeader>() == 8);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct OpReqImport {
    pub header: OpHeader,
    pub busid: [u8; 32],
}
const _: () = assert!(size_of::<OpReqImport>() == 40);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct OpRepDevlistHeader {
    pub header: OpHeader,
    pub num_devices: U32,
}
const _: () = assert!(size_of::<OpRepDevlistHeader>() == 12);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct WireDeviceInfo {
    pub path: [u8; 256],
    pub busid: [u8; 32],
    pub busnum: U32,
    pub devnum: U32,
    pub speed: U32,
    pub id_vendor: U16,
    pub id_product: U16,
    pub bcd_device: U16,
    pub device_class: u8,
    pub device_sub_class: u8,
    pub device_protocol: u8,
    pub configuration_value: u8,
    pub num_configurations: u8,
    pub num_interfaces: u8,
}
const _: () = assert!(size_of::<WireDeviceInfo>() == 312);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct WireInterfaceInfo {
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub _padding: u8,
}
const _: () = assert!(size_of::<WireInterfaceInfo>() == 4);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct CmdSubmitHeader {
    pub command: U32,
    pub seqnum: U32,
    pub devid: U32,
    pub direction: U32,
    pub ep: U32,
    pub transfer_flags: U32,
    pub transfer_buffer_length: U32,
    pub start_frame: U32,
    pub number_of_packets: U32,
    pub interval: U32,
    pub setup: [u8; 8],
}
const _: () = assert!(size_of::<CmdSubmitHeader>() == 48);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct CmdUnlinkHeader {
    pub command: U32,
    pub seqnum: U32,
    pub devid: U32,
    pub direction: U32,
    pub ep: U32,
    pub unlink_seqnum: U32,
    pub _padding: [u8; 24],
}
const _: () = assert!(size_of::<CmdUnlinkHeader>() == 48);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct RetSubmitHeader {
    pub command: U32,
    pub seqnum: U32,
    pub devid: U32,
    pub direction: U32,
    pub ep: U32,
    pub status: I32,
    pub actual_length: U32,
    pub start_frame: U32,
    pub number_of_packets: U32,
    pub error_count: U32,
    pub _padding: [u8; 8],
}
const _: () = assert!(size_of::<RetSubmitHeader>() == 48);

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct RetUnlinkHeader {
    pub command: U32,
    pub seqnum: U32,
    pub devid: U32,
    pub direction: U32,
    pub ep: U32,
    pub status: I32,
    pub _padding: [u8; 24],
}
const _: () = assert!(size_of::<RetUnlinkHeader>() == 48);

// --- Conversions ---

impl From<&WireDeviceInfo> for DeviceInfo {
    fn from(w: &WireDeviceInfo) -> Self {
        DeviceInfo {
            path: bytes_to_string(&w.path),
            busid: bytes_to_string(&w.busid),
            busnum: w.busnum.get(),
            devnum: w.devnum.get(),
            speed: w.speed.get(),
            id_vendor: w.id_vendor.get(),
            id_product: w.id_product.get(),
            bcd_device: w.bcd_device.get(),
            device_class: w.device_class,
            device_sub_class: w.device_sub_class,
            device_protocol: w.device_protocol,
            configuration_value: w.configuration_value,
            num_configurations: w.num_configurations,
            num_interfaces: w.num_interfaces,
            interfaces: Vec::new(),
        }
    }
}

impl From<&WireInterfaceInfo> for InterfaceInfo {
    fn from(w: &WireInterfaceInfo) -> Self {
        InterfaceInfo {
            class: w.class,
            sub_class: w.sub_class,
            protocol: w.protocol,
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::IntoBytes;

    #[test]
    fn test_cmd_submit_parse_known_bytes() {
        // From usbip_protocol.rst EXAMPLE: CmdIntrIN
        let bytes: [u8; 48] = [
            0x00, 0x00, 0x00, 0x01, // command = CMD_SUBMIT (1)
            0x00, 0x00, 0x0d, 0x05, // seqnum = 0x0d05
            0x00, 0x01, 0x00, 0x0f, // devid = 0x1000f
            0x00, 0x00, 0x00, 0x01, // direction = IN (1)
            0x00, 0x00, 0x00, 0x01, // ep = 1
            0x00, 0x00, 0x02, 0x00, // transfer_flags = 0x200
            0x00, 0x00, 0x00, 0x40, // transfer_buffer_length = 64
            0xff, 0xff, 0xff, 0xff, // start_frame = 0xffffffff
            0x00, 0x00, 0x00, 0x00, // number_of_packets = 0
            0x00, 0x00, 0x00, 0x04, // interval = 4
            0x00, 0x00, 0x00, 0x00, // setup[0..4]
            0x00, 0x00, 0x00, 0x00, // setup[4..8]
        ];

        let header = CmdSubmitHeader::ref_from_bytes(&bytes).unwrap();
        assert_eq!(header.command.get(), 1);
        assert_eq!(header.seqnum.get(), 0x0d05);
        assert_eq!(header.devid.get(), 0x0001_000f);
        assert_eq!(header.direction.get(), 1);
        assert_eq!(header.ep.get(), 1);
        assert_eq!(header.transfer_flags.get(), 0x200);
        assert_eq!(header.transfer_buffer_length.get(), 64);
        assert_eq!(header.start_frame.get(), 0xffffffff);
        assert_eq!(header.number_of_packets.get(), 0);
        assert_eq!(header.interval.get(), 4);
        assert_eq!(header.setup, [0; 8]);
    }

    #[test]
    fn test_ret_submit_parse_known_bytes() {
        // From usbip_protocol.rst EXAMPLE: RetIntrOut
        let bytes: [u8; 48] = [
            0x00, 0x00, 0x00, 0x03, // command = RET_SUBMIT (3)
            0x00, 0x00, 0x0d, 0x06, // seqnum = 0x0d06
            0x00, 0x00, 0x00, 0x00, // devid = 0
            0x00, 0x00, 0x00, 0x00, // direction = 0
            0x00, 0x00, 0x00, 0x00, // ep = 0
            0x00, 0x00, 0x00, 0x00, // status = 0
            0x00, 0x00, 0x00, 0x40, // actual_length = 64
            0xff, 0xff, 0xff, 0xff, // start_frame = 0xffffffff
            0x00, 0x00, 0x00, 0x00, // number_of_packets = 0
            0x00, 0x00, 0x00, 0x00, // error_count = 0
            0x00, 0x00, 0x00, 0x00, // padding[0..4]
            0x00, 0x00, 0x00, 0x00, // padding[4..8]
        ];

        let header = RetSubmitHeader::ref_from_bytes(&bytes).unwrap();
        assert_eq!(header.command.get(), 3);
        assert_eq!(header.seqnum.get(), 0x0d06);
        assert_eq!(header.devid.get(), 0);
        assert_eq!(header.direction.get(), 0);
        assert_eq!(header.ep.get(), 0);
        assert_eq!(header.status.get(), 0);
        assert_eq!(header.actual_length.get(), 64);
        assert_eq!(header.start_frame.get(), 0xffffffff);
        assert_eq!(header.number_of_packets.get(), 0);
        assert_eq!(header.error_count.get(), 0);
    }

    #[test]
    fn test_cmd_submit_roundtrip() {
        let header = CmdSubmitHeader {
            command: U32::new(1),
            seqnum: U32::new(42),
            devid: U32::new(0x0002_0003),
            direction: U32::new(0),
            ep: U32::new(2),
            transfer_flags: U32::new(0x200),
            transfer_buffer_length: U32::new(512),
            start_frame: U32::new(0),
            number_of_packets: U32::new(0xffffffff),
            interval: U32::new(0),
            setup: [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x40, 0x00],
        };

        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), 48);

        let parsed = CmdSubmitHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(parsed.command.get(), 1);
        assert_eq!(parsed.seqnum.get(), 42);
        assert_eq!(parsed.devid.get(), 0x0002_0003);
        assert_eq!(parsed.direction.get(), 0);
        assert_eq!(parsed.ep.get(), 2);
        assert_eq!(parsed.transfer_flags.get(), 0x200);
        assert_eq!(parsed.transfer_buffer_length.get(), 512);
        assert_eq!(parsed.start_frame.get(), 0);
        assert_eq!(parsed.number_of_packets.get(), 0xffffffff);
        assert_eq!(parsed.interval.get(), 0);
        assert_eq!(
            parsed.setup,
            [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x40, 0x00]
        );
    }

    #[test]
    fn test_ret_submit_roundtrip() {
        let header = RetSubmitHeader {
            command: U32::new(3),
            seqnum: U32::new(100),
            devid: U32::new(0),
            direction: U32::new(0),
            ep: U32::new(0),
            status: I32::new(-1),
            actual_length: U32::new(0),
            start_frame: U32::new(0),
            number_of_packets: U32::new(0xffffffff),
            error_count: U32::new(0),
            _padding: [0; 8],
        };

        let bytes = header.as_bytes();
        let parsed = RetSubmitHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(parsed.status.get(), -1);
        assert_eq!(parsed.actual_length.get(), 0);
    }

    #[test]
    fn test_cmd_unlink_roundtrip() {
        let header = CmdUnlinkHeader {
            command: U32::new(2),
            seqnum: U32::new(50),
            devid: U32::new(0x0001_0002),
            direction: U32::new(0),
            ep: U32::new(0),
            unlink_seqnum: U32::new(49),
            _padding: [0; 24],
        };

        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), 48);

        let parsed = CmdUnlinkHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(parsed.command.get(), 2);
        assert_eq!(parsed.seqnum.get(), 50);
        assert_eq!(parsed.unlink_seqnum.get(), 49);
    }

    #[test]
    fn test_ret_unlink_roundtrip() {
        let header = RetUnlinkHeader {
            command: U32::new(4),
            seqnum: U32::new(50),
            devid: U32::new(0),
            direction: U32::new(0),
            ep: U32::new(0),
            status: I32::new(-104), // -ECONNRESET
            _padding: [0; 24],
        };

        let bytes = header.as_bytes();
        let parsed = RetUnlinkHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(parsed.command.get(), 4);
        assert_eq!(parsed.status.get(), -104);
    }

    #[test]
    fn test_op_header_roundtrip() {
        let header = OpHeader {
            version: U16::new(0x0111),
            code: U16::new(0x8005),
            status: U32::new(0),
        };

        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), 8);

        let parsed = OpHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(parsed.version.get(), 0x0111);
        assert_eq!(parsed.code.get(), 0x8005);
        assert_eq!(parsed.status.get(), 0);
    }

    #[test]
    fn test_op_req_import_roundtrip() {
        let mut req = OpReqImport {
            header: OpHeader {
                version: U16::new(0x0111),
                code: U16::new(0x8003),
                status: U32::new(0),
            },
            busid: [0u8; 32],
        };
        let busid = b"1-1";
        req.busid[..busid.len()].copy_from_slice(busid);

        let bytes = req.as_bytes();
        assert_eq!(bytes.len(), 40);

        let parsed = OpReqImport::ref_from_bytes(bytes).unwrap();
        assert_eq!(parsed.header.version.get(), 0x0111);
        assert_eq!(parsed.header.code.get(), 0x8003);
        assert_eq!(&parsed.busid[..3], b"1-1");
        assert_eq!(parsed.busid[3], 0);
    }

    #[test]
    fn test_wire_device_info_to_device_info() {
        let mut wire = WireDeviceInfo {
            path: [0u8; 256],
            busid: [0u8; 32],
            busnum: U32::new(3),
            devnum: U32::new(2),
            speed: U32::new(2),
            id_vendor: U16::new(0x1234),
            id_product: U16::new(0x5678),
            bcd_device: U16::new(0x0100),
            device_class: 0xff,
            device_sub_class: 0x01,
            device_protocol: 0x02,
            configuration_value: 1,
            num_configurations: 1,
            num_interfaces: 2,
        };
        let path = b"/sys/devices/pci0000:00/usb3/3-2";
        wire.path[..path.len()].copy_from_slice(path);
        let busid = b"3-2";
        wire.busid[..busid.len()].copy_from_slice(busid);

        let dev = DeviceInfo::from(&wire);
        assert_eq!(dev.path, "/sys/devices/pci0000:00/usb3/3-2");
        assert_eq!(dev.busid, "3-2");
        assert_eq!(dev.busnum, 3);
        assert_eq!(dev.devnum, 2);
        assert_eq!(dev.id_vendor, 0x1234);
        assert_eq!(dev.id_product, 0x5678);
        assert_eq!(dev.num_interfaces, 2);
        assert!(dev.interfaces.is_empty());
    }

    #[test]
    fn test_wire_interface_info_to_interface_info() {
        let wire = WireInterfaceInfo {
            class: 0x03,
            sub_class: 0x01,
            protocol: 0x02,
            _padding: 0,
        };

        let iface = InterfaceInfo::from(&wire);
        assert_eq!(iface.class, 0x03);
        assert_eq!(iface.sub_class, 0x01);
        assert_eq!(iface.protocol, 0x02);
    }
}
