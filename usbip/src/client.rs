use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};

use zerocopy::byteorder::network_endian::{U16, U32};
use zerocopy::{FromBytes, IntoBytes};

use crate::protocol::*;
use crate::wire::*;

/// Lists USB devices exported by the USB/IP server at `addr`.
pub fn list_devices(addr: impl ToSocketAddrs) -> io::Result<Vec<DeviceInfo>> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;

    // Send OP_REQ_DEVLIST
    let header = OpHeader {
        version: U16::new(USBIP_VERSION),
        code: U16::new(OP_REQ_DEVLIST),
        status: U32::new(0),
    };
    stream.write_all(header.as_bytes())?;
    stream.flush()?;

    // Read OP_REP_DEVLIST header
    let rep: OpRepDevlistHeader = read_wire(&mut stream)?;
    let code = rep.header.code.get();
    let status = rep.header.status.get();

    if code != OP_REP_DEVLIST {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected reply code: 0x{code:04x}"),
        ));
    }
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("server returned error status: {status}"),
        ));
    }

    let num_devices = rep.num_devices.get();
    let mut devices = Vec::with_capacity(num_devices as usize);

    for _ in 0..num_devices {
        let wire_dev: WireDeviceInfo = read_wire(&mut stream)?;
        let num_ifaces = wire_dev.num_interfaces;
        let mut dev = DeviceInfo::from(&wire_dev);
        for _ in 0..num_ifaces {
            let wire_iface: WireInterfaceInfo = read_wire(&mut stream)?;
            dev.interfaces.push(InterfaceInfo::from(&wire_iface));
        }
        devices.push(dev);
    }

    Ok(devices)
}

/// Imports a USB device from the USB/IP server at `addr`.
///
/// Returns the device information and an active connection for URB transfer.
pub fn import_device(
    addr: impl ToSocketAddrs,
    busid: &str,
) -> io::Result<(DeviceInfo, UsbipConnection)> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;

    // Send OP_REQ_IMPORT
    let mut req = OpReqImport {
        header: OpHeader {
            version: U16::new(USBIP_VERSION),
            code: U16::new(OP_REQ_IMPORT),
            status: U32::new(0),
        },
        busid: [0u8; BUSID_LEN],
    };
    let busid_bytes = busid.as_bytes();
    let copy_len = busid_bytes.len().min(BUSID_LEN - 1);
    req.busid[..copy_len].copy_from_slice(&busid_bytes[..copy_len]);
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    // Read OP_REP_IMPORT header
    let rep: OpHeader = read_wire(&mut stream)?;
    let code = rep.code.get();
    let status = rep.status.get();

    if code != OP_REP_IMPORT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected reply code: 0x{code:04x}"),
        ));
    }
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("import failed with status: {status}"),
        ));
    }

    // Read device info
    let wire_dev: WireDeviceInfo = read_wire(&mut stream)?;
    let dev = DeviceInfo::from(&wire_dev);
    let devid = (dev.busnum << 16) | dev.devnum;

    let conn = UsbipConnection {
        stream,
        devid,
        next_seqnum: 1,
        pending: HashMap::new(),
        unlinks: HashMap::new(),
    };

    Ok((dev, conn))
}

/// An active USB/IP connection for URB transfer.
pub struct UsbipConnection {
    stream: TcpStream,
    devid: u32,
    next_seqnum: u32,
    pending: HashMap<u32, Direction>,
    unlinks: HashMap<u32, u32>,
}

// ECONNRESET on Linux
const ECONNRESET: i32 = 104;

impl UsbipConnection {
    fn alloc_seqnum(&mut self) -> u32 {
        let seq = self.next_seqnum;
        self.next_seqnum += 1;
        seq
    }

    /// Submits a URB to the remote device.
    ///
    /// Returns the sequence number assigned to this submission.
    pub fn send_submit(
        &mut self,
        ep: u32,
        direction: Direction,
        transfer_flags: u32,
        transfer_buffer_length: u32,
        setup: [u8; 8],
        data: &[u8],
        interval: u32,
    ) -> io::Result<u32> {
        let seqnum = self.alloc_seqnum();

        write_cmd_submit(
            &mut self.stream,
            seqnum,
            self.devid,
            ep,
            direction,
            transfer_flags,
            transfer_buffer_length,
            setup,
            data,
            interval,
        )?;

        self.pending.insert(seqnum, direction);
        Ok(seqnum)
    }

    /// Sends an unlink request for a previously submitted URB.
    ///
    /// Returns the sequence number assigned to this unlink command.
    pub fn send_unlink(&mut self, unlink_seqnum: u32) -> io::Result<u32> {
        let seqnum = self.alloc_seqnum();

        write_cmd_unlink(&mut self.stream, seqnum, self.devid, unlink_seqnum)?;

        self.unlinks.insert(seqnum, unlink_seqnum);
        Ok(seqnum)
    }

    /// Receives the next URB response from the server.
    pub fn recv(&mut self) -> io::Result<UrbResponse> {
        recv_urb_response(&mut self.stream, &mut self.pending, &mut self.unlinks)
    }

    /// Returns a reference to the underlying TCP stream.
    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }

    /// Splits this connection into independent reader and writer halves.
    ///
    /// The writer can be wrapped in `Arc<Mutex<>>` for multi-thread access.
    /// The reader is typically owned by a dedicated reader thread.
    pub fn into_split(self) -> io::Result<(UsbipReader, UsbipWriter)> {
        let read_stream = self.stream.try_clone()?;
        let shared = Arc::new(Mutex::new(SharedState {
            pending: self.pending,
            unlinks: self.unlinks,
        }));
        let reader = UsbipReader {
            stream: read_stream,
            shared: Arc::clone(&shared),
        };
        let writer = UsbipWriter {
            stream: self.stream,
            devid: self.devid,
            next_seqnum: self.next_seqnum,
            shared,
        };
        Ok((reader, writer))
    }
}

// --- Shared state for split halves ---

struct SharedState {
    pending: HashMap<u32, Direction>,
    unlinks: HashMap<u32, u32>,
}

/// The writer half of a split USB/IP connection.
///
/// Encodes and sends CMD_SUBMIT / CMD_UNLINK.
/// Wrap in `Arc<Mutex<UsbipWriter>>` for multi-thread access.
pub struct UsbipWriter {
    stream: TcpStream,
    devid: u32,
    next_seqnum: u32,
    shared: Arc<Mutex<SharedState>>,
}

impl UsbipWriter {
    fn alloc_seqnum(&mut self) -> u32 {
        let seq = self.next_seqnum;
        self.next_seqnum += 1;
        seq
    }

    /// Submits a URB to the remote device.
    ///
    /// Returns the sequence number assigned to this submission.
    pub fn send_submit(
        &mut self,
        ep: u32,
        direction: Direction,
        transfer_flags: u32,
        transfer_buffer_length: u32,
        setup: [u8; 8],
        data: &[u8],
        interval: u32,
    ) -> io::Result<u32> {
        let seqnum = self.alloc_seqnum();

        // Register pending BEFORE writing to avoid race with reader
        {
            let mut shared = self.shared.lock().unwrap();
            shared.pending.insert(seqnum, direction);
        }

        if let Err(e) = write_cmd_submit(
            &mut self.stream,
            seqnum,
            self.devid,
            ep,
            direction,
            transfer_flags,
            transfer_buffer_length,
            setup,
            data,
            interval,
        ) {
            // Clean up pending on write failure
            let mut shared = self.shared.lock().unwrap();
            shared.pending.remove(&seqnum);
            return Err(e);
        }

        Ok(seqnum)
    }

    /// Sends an unlink request for a previously submitted URB.
    ///
    /// Returns the sequence number assigned to this unlink command.
    pub fn send_unlink(&mut self, unlink_seqnum: u32) -> io::Result<u32> {
        let seqnum = self.alloc_seqnum();

        {
            let mut shared = self.shared.lock().unwrap();
            shared.unlinks.insert(seqnum, unlink_seqnum);
        }

        if let Err(e) = write_cmd_unlink(&mut self.stream, seqnum, self.devid, unlink_seqnum) {
            let mut shared = self.shared.lock().unwrap();
            shared.unlinks.remove(&seqnum);
            return Err(e);
        }

        Ok(seqnum)
    }
}

/// The reader half of a split USB/IP connection.
///
/// Decodes RET_SUBMIT / RET_UNLINK responses.
/// Typically owned by a dedicated reader thread.
pub struct UsbipReader {
    stream: TcpStream,
    shared: Arc<Mutex<SharedState>>,
}

impl UsbipReader {
    /// Receives the next URB response from the server.
    pub fn recv(&mut self) -> io::Result<UrbResponse> {
        let mut buf = [0u8; 48];
        self.stream.read_exact(&mut buf)?;

        let command = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);

        match command {
            USBIP_RET_SUBMIT => {
                let header = RetSubmitHeader::ref_from_bytes(&buf)
                    .expect("buf is exactly 48 bytes with alignment 1");
                let seqnum = header.seqnum.get();
                let status = header.status.get();
                let actual_length = header.actual_length.get();

                let direction = {
                    let mut shared = self.shared.lock().unwrap();
                    shared.pending.remove(&seqnum)
                }
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("RET_SUBMIT for unknown seqnum {seqnum}"),
                    )
                })?;

                let data = if direction == Direction::In && actual_length > 0 {
                    let mut buf = vec![0u8; actual_length as usize];
                    self.stream.read_exact(&mut buf)?;
                    buf
                } else {
                    Vec::new()
                };

                Ok(UrbResponse::Submit(RetSubmit {
                    seqnum,
                    status,
                    actual_length,
                    data,
                    direction,
                }))
            }
            USBIP_RET_UNLINK => {
                let header = RetUnlinkHeader::ref_from_bytes(&buf)
                    .expect("buf is exactly 48 bytes with alignment 1");
                let seqnum = header.seqnum.get();
                let status = header.status.get();

                {
                    let mut shared = self.shared.lock().unwrap();
                    if let Some(target_seqnum) = shared.unlinks.remove(&seqnum) {
                        if status == -ECONNRESET {
                            shared.pending.remove(&target_seqnum);
                        }
                    }
                }

                Ok(UrbResponse::Unlink(RetUnlink { seqnum, status }))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown URB response command: {command}"),
            )),
        }
    }
}

// --- Wire format helpers ---

fn write_cmd_submit(
    w: &mut impl Write,
    seqnum: u32,
    devid: u32,
    ep: u32,
    direction: Direction,
    transfer_flags: u32,
    transfer_buffer_length: u32,
    setup: [u8; 8],
    data: &[u8],
    interval: u32,
) -> io::Result<()> {
    let header = CmdSubmitHeader {
        command: U32::new(USBIP_CMD_SUBMIT),
        seqnum: U32::new(seqnum),
        devid: U32::new(devid),
        direction: U32::new(direction as u32),
        ep: U32::new(ep),
        transfer_flags: U32::new(transfer_flags),
        transfer_buffer_length: U32::new(transfer_buffer_length),
        start_frame: U32::new(0),
        number_of_packets: U32::new(0xffffffff),
        interval: U32::new(interval),
        setup,
    };
    w.write_all(header.as_bytes())?;

    // transfer_buffer for OUT direction
    if direction == Direction::Out && !data.is_empty() {
        w.write_all(data)?;
    }

    w.flush()?;
    Ok(())
}

fn write_cmd_unlink(
    w: &mut impl Write,
    seqnum: u32,
    devid: u32,
    unlink_seqnum: u32,
) -> io::Result<()> {
    let header = CmdUnlinkHeader {
        command: U32::new(USBIP_CMD_UNLINK),
        seqnum: U32::new(seqnum),
        devid: U32::new(devid),
        direction: U32::new(0),
        ep: U32::new(0),
        unlink_seqnum: U32::new(unlink_seqnum),
        _padding: [0u8; 24],
    };
    w.write_all(header.as_bytes())?;

    w.flush()?;
    Ok(())
}

fn recv_urb_response(
    r: &mut impl Read,
    pending: &mut HashMap<u32, Direction>,
    unlinks: &mut HashMap<u32, u32>,
) -> io::Result<UrbResponse> {
    let mut buf = [0u8; 48];
    r.read_exact(&mut buf)?;

    let command = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);

    match command {
        USBIP_RET_SUBMIT => {
            let header = RetSubmitHeader::ref_from_bytes(&buf)
                .expect("buf is exactly 48 bytes with alignment 1");
            let seqnum = header.seqnum.get();
            let status = header.status.get();
            let actual_length = header.actual_length.get();

            let direction = pending.remove(&seqnum).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("RET_SUBMIT for unknown seqnum {seqnum}"),
                )
            })?;

            let data = if direction == Direction::In && actual_length > 0 {
                let mut buf = vec![0u8; actual_length as usize];
                r.read_exact(&mut buf)?;
                buf
            } else {
                Vec::new()
            };

            Ok(UrbResponse::Submit(RetSubmit {
                seqnum,
                status,
                actual_length,
                data,
                direction,
            }))
        }
        USBIP_RET_UNLINK => {
            let header = RetUnlinkHeader::ref_from_bytes(&buf)
                .expect("buf is exactly 48 bytes with alignment 1");
            let seqnum = header.seqnum.get();
            let status = header.status.get();

            if let Some(target_seqnum) = unlinks.remove(&seqnum) {
                if status == -ECONNRESET {
                    pending.remove(&target_seqnum);
                }
            }

            Ok(UrbResponse::Unlink(RetUnlink { seqnum, status }))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown URB response command: {command}"),
        )),
    }
}
