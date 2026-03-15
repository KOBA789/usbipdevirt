use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

use crate::protocol::*;

/// Lists USB devices exported by the USB/IP server at `addr`.
pub fn list_devices(addr: impl ToSocketAddrs) -> io::Result<Vec<DeviceInfo>> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;

    // Send OP_REQ_DEVLIST (8 bytes)
    write_u16_be(&mut stream, USBIP_VERSION)?;
    write_u16_be(&mut stream, OP_REQ_DEVLIST)?;
    write_u32_be(&mut stream, 0)?;
    stream.flush()?;

    // Read OP_REP_DEVLIST header (12 bytes)
    let _version = read_u16_be(&mut stream)?;
    let code = read_u16_be(&mut stream)?;
    let status = read_u32_be(&mut stream)?;

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

    let num_devices = read_u32_be(&mut stream)?;
    let mut devices = Vec::with_capacity(num_devices as usize);

    for _ in 0..num_devices {
        let mut dev = read_device_info(&mut stream)?;
        let num_ifaces = dev.num_interfaces;
        for _ in 0..num_ifaces {
            dev.interfaces.push(read_interface_info(&mut stream)?);
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

    // Send OP_REQ_IMPORT (40 bytes: 8 header + 32 busid)
    write_u16_be(&mut stream, USBIP_VERSION)?;
    write_u16_be(&mut stream, OP_REQ_IMPORT)?;
    write_u32_be(&mut stream, 0)?;

    let mut busid_buf = [0u8; BUSID_LEN];
    let busid_bytes = busid.as_bytes();
    let copy_len = busid_bytes.len().min(BUSID_LEN - 1);
    busid_buf[..copy_len].copy_from_slice(&busid_bytes[..copy_len]);
    stream.write_all(&busid_buf)?;
    stream.flush()?;

    // Read OP_REP_IMPORT header (8 bytes)
    let _version = read_u16_be(&mut stream)?;
    let code = read_u16_be(&mut stream)?;
    let status = read_u32_be(&mut stream)?;

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

    // Read device info (312 bytes, no interfaces in import reply)
    let dev = read_device_info(&mut stream)?;
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

        // usbip_header_basic (20 bytes)
        write_u32_be(&mut self.stream, USBIP_CMD_SUBMIT)?;
        write_u32_be(&mut self.stream, seqnum)?;
        write_u32_be(&mut self.stream, self.devid)?;
        write_u32_be(&mut self.stream, direction as u32)?;
        write_u32_be(&mut self.stream, ep)?;

        // CMD_SUBMIT specific (28 bytes)
        write_u32_be(&mut self.stream, transfer_flags)?;
        write_u32_be(&mut self.stream, transfer_buffer_length)?;
        write_u32_be(&mut self.stream, 0)?; // start_frame
        write_u32_be(&mut self.stream, 0xffffffff)?; // number_of_packets (non-ISO)
        write_u32_be(&mut self.stream, interval)?;
        self.stream.write_all(&setup)?;

        // transfer_buffer for OUT direction
        if direction == Direction::Out && !data.is_empty() {
            self.stream.write_all(data)?;
        }

        self.stream.flush()?;
        self.pending.insert(seqnum, direction);
        Ok(seqnum)
    }

    /// Sends an unlink request for a previously submitted URB.
    ///
    /// Returns the sequence number assigned to this unlink command.
    pub fn send_unlink(&mut self, unlink_seqnum: u32) -> io::Result<u32> {
        let seqnum = self.alloc_seqnum();

        // usbip_header_basic (20 bytes)
        write_u32_be(&mut self.stream, USBIP_CMD_UNLINK)?;
        write_u32_be(&mut self.stream, seqnum)?;
        write_u32_be(&mut self.stream, self.devid)?;
        write_u32_be(&mut self.stream, 0)?; // direction
        write_u32_be(&mut self.stream, 0)?; // ep

        // CMD_UNLINK specific (28 bytes)
        write_u32_be(&mut self.stream, unlink_seqnum)?;
        self.stream.write_all(&[0u8; 24])?; // padding

        self.stream.flush()?;
        self.unlinks.insert(seqnum, unlink_seqnum);
        Ok(seqnum)
    }

    /// Receives the next URB response from the server.
    pub fn recv(&mut self) -> io::Result<UrbResponse> {
        // Read the full 48-byte header
        let header: [u8; 48] = read_exact_array(&mut self.stream)?;

        let command = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let seqnum = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

        match command {
            USBIP_RET_SUBMIT => {
                let status =
                    i32::from_be_bytes([header[20], header[21], header[22], header[23]]);
                let actual_length =
                    u32::from_be_bytes([header[24], header[25], header[26], header[27]]);

                let direction = self.pending.remove(&seqnum).ok_or_else(|| {
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
                let status =
                    i32::from_be_bytes([header[20], header[21], header[22], header[23]]);

                if let Some(target_seqnum) = self.unlinks.remove(&seqnum) {
                    if status == -ECONNRESET {
                        self.pending.remove(&target_seqnum);
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

    /// Returns a reference to the underlying TCP stream.
    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }
}
