use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};

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
        let command = read_u32_be(&mut self.stream)?;
        let seqnum = read_u32_be(&mut self.stream)?;
        // devid (4) + direction (4) + ep (4)
        let _: [u8; 12] = read_exact_array(&mut self.stream)?;

        match command {
            USBIP_RET_SUBMIT => {
                let status = read_i32_be(&mut self.stream)?;
                let actual_length = read_u32_be(&mut self.stream)?;
                // start_frame (4) + number_of_packets (4) + error_count (4) + setup (8)
                let _: [u8; 20] = read_exact_array(&mut self.stream)?;

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
                let status = read_i32_be(&mut self.stream)?;
                // padding (24)
                let _: [u8; 24] = read_exact_array(&mut self.stream)?;

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
            _ => {
                // Drain remaining 28 bytes of the header
                let _: [u8; 28] = read_exact_array(&mut self.stream)?;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown URB response command: {command}"),
                ))
            }
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
    // usbip_header_basic (20 bytes)
    write_u32_be(w, USBIP_CMD_SUBMIT)?;
    write_u32_be(w, seqnum)?;
    write_u32_be(w, devid)?;
    write_u32_be(w, direction as u32)?;
    write_u32_be(w, ep)?;

    // CMD_SUBMIT specific (28 bytes)
    write_u32_be(w, transfer_flags)?;
    write_u32_be(w, transfer_buffer_length)?;
    write_u32_be(w, 0)?; // start_frame
    write_u32_be(w, 0xffffffff)?; // number_of_packets (non-ISO)
    write_u32_be(w, interval)?;
    w.write_all(&setup)?;

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
    // usbip_header_basic (20 bytes)
    write_u32_be(w, USBIP_CMD_UNLINK)?;
    write_u32_be(w, seqnum)?;
    write_u32_be(w, devid)?;
    write_u32_be(w, 0)?; // direction
    write_u32_be(w, 0)?; // ep

    // CMD_UNLINK specific (28 bytes)
    write_u32_be(w, unlink_seqnum)?;
    w.write_all(&[0u8; 24])?; // padding

    w.flush()?;
    Ok(())
}

fn recv_urb_response(
    r: &mut impl Read,
    pending: &mut HashMap<u32, Direction>,
    unlinks: &mut HashMap<u32, u32>,
) -> io::Result<UrbResponse> {
    // Read the full 48-byte header
    let header: [u8; 48] = read_exact_array(r)?;

    let command = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let seqnum = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

    match command {
        USBIP_RET_SUBMIT => {
            let status = i32::from_be_bytes([header[20], header[21], header[22], header[23]]);
            let actual_length =
                u32::from_be_bytes([header[24], header[25], header[26], header[27]]);

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
            let status = i32::from_be_bytes([header[20], header[21], header[22], header[23]]);

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
