use std::fs::{File, OpenOptions};
use std::io;
use std::mem::size_of;
use std::os::fd::{AsRawFd, RawFd};

use crate::ep::EpHandle;
use crate::event::{
    self, Event,
};
use crate::types::{
    UDC_NAME_LENGTH_MAX, UsbRawEpIoHeader, UsbRawEpsInfo, UsbRawEventHeader, UsbRawInit,
};
use crate::usb_types::{UsbCtrlRequest, UsbEndpointDescriptor, UsbSpeed};
use crate::{ioctl, types};

const EP0_BUF_SIZE: usize = 512;

#[repr(C)]
struct Ep0IoBuf {
    header: UsbRawEpIoHeader,
    data: [u8; EP0_BUF_SIZE],
}

impl Ep0IoBuf {
    fn zeroed() -> Self {
        Self {
            header: UsbRawEpIoHeader {
                ep: 0,
                flags: 0,
                length: 0,
            },
            data: [0u8; EP0_BUF_SIZE],
        }
    }
}

/// Safe wrapper around a `/dev/raw-gadget` file descriptor.
///
/// All methods take `&self` — the kernel handles internal synchronization,
/// so the device can be shared via `Arc` across threads.
pub struct RawGadgetDevice {
    file: File,
}

impl RawGadgetDevice {
    /// Open `/dev/raw-gadget`.
    pub fn open() -> io::Result<Self> {
        Self::open_path("/dev/raw-gadget")
    }

    /// Open a raw-gadget device at the given path.
    pub fn open_path(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self { file })
    }

    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Initialize the raw-gadget instance (`USB_RAW_IOCTL_INIT`).
    pub fn init(&self, driver_name: &str, device_name: &str, speed: UsbSpeed) -> io::Result<()> {
        if driver_name.len() >= UDC_NAME_LENGTH_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "driver name too long",
            ));
        }
        if device_name.len() >= UDC_NAME_LENGTH_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "device name too long",
            ));
        }
        let mut raw = UsbRawInit {
            driver_name: [0u8; UDC_NAME_LENGTH_MAX],
            device_name: [0u8; UDC_NAME_LENGTH_MAX],
            speed: speed as u8,
        };
        raw.driver_name[..driver_name.len()].copy_from_slice(driver_name.as_bytes());
        raw.device_name[..device_name.len()].copy_from_slice(device_name.as_bytes());
        unsafe { ioctl::ioctl_init(self.fd(), &raw) }?;
        Ok(())
    }

    /// Start the gadget (`USB_RAW_IOCTL_RUN`).
    pub fn run(&self) -> io::Result<()> {
        unsafe { ioctl::ioctl_run(self.fd()) }?;
        Ok(())
    }

    /// Fetch the next event (blocking) (`USB_RAW_IOCTL_EVENT_FETCH`).
    pub fn event_fetch(&self) -> io::Result<Event> {
        #[repr(C)]
        struct EventFetchBuf {
            header: UsbRawEventHeader,
            ctrl: UsbCtrlRequest,
        }

        let mut buf = EventFetchBuf {
            header: UsbRawEventHeader {
                event_type: 0,
                length: size_of::<UsbCtrlRequest>() as u32,
            },
            ctrl: UsbCtrlRequest {
                bRequestType: 0,
                bRequest: 0,
                wValue: 0,
                wIndex: 0,
                wLength: 0,
            },
        };

        unsafe { ioctl::ioctl_event_fetch(self.fd(), &mut buf.header) }?;

        let event = match buf.header.event_type {
            event::USB_RAW_EVENT_CONNECT => Event::Connect,
            event::USB_RAW_EVENT_CONTROL => Event::Control(buf.ctrl),
            event::USB_RAW_EVENT_SUSPEND => Event::Suspend,
            event::USB_RAW_EVENT_RESUME => Event::Resume,
            event::USB_RAW_EVENT_RESET => Event::Reset,
            event::USB_RAW_EVENT_DISCONNECT => Event::Disconnect,
            t => Event::Unknown(t),
        };
        Ok(event)
    }

    /// Write data to endpoint 0 (`USB_RAW_IOCTL_EP0_WRITE`).
    ///
    /// Returns the number of bytes transferred.
    pub fn ep0_write(&self, data: &[u8]) -> io::Result<usize> {
        if data.len() > EP0_BUF_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "ep0_write: data too large ({} > {})",
                    data.len(),
                    EP0_BUF_SIZE
                ),
            ));
        }

        let mut buf = Ep0IoBuf::zeroed();
        buf.header.length = data.len() as u32;
        buf.data[..data.len()].copy_from_slice(data);

        unsafe { ioctl::ioctl_ep0_write(self.fd(), &buf.header) }.map(|v| v as usize)
    }

    /// Read data from endpoint 0 (`USB_RAW_IOCTL_EP0_READ`).
    ///
    /// Returns the number of bytes transferred.
    pub fn ep0_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let len = buf.len().min(EP0_BUF_SIZE);
        let mut io_buf = Ep0IoBuf::zeroed();
        io_buf.header.length = len as u32;

        let transferred =
            unsafe { ioctl::ioctl_ep0_read(self.fd(), &mut io_buf.header) }? as usize;
        let n = transferred.min(len);
        buf[..n].copy_from_slice(&io_buf.data[..n]);
        Ok(n)
    }

    /// Enable a non-control endpoint (`USB_RAW_IOCTL_EP_ENABLE`).
    pub fn ep_enable(&self, desc: &UsbEndpointDescriptor) -> io::Result<EpHandle> {
        let handle = unsafe { ioctl::ioctl_ep_enable(self.fd(), desc) }?;
        Ok(EpHandle(handle as u32))
    }

    /// Disable a non-control endpoint (`USB_RAW_IOCTL_EP_DISABLE`).
    pub fn ep_disable(&self, ep: EpHandle) -> io::Result<()> {
        unsafe { ioctl::ioctl_ep_disable(self.fd(), ep.0) }?;
        Ok(())
    }

    /// Write data to a non-control endpoint (`USB_RAW_IOCTL_EP_WRITE`).
    ///
    /// Uses heap allocation for the I/O buffer (supports up to 64 KB+).
    /// Returns the number of bytes transferred.
    pub fn ep_write(&self, ep: EpHandle, data: &[u8]) -> io::Result<usize> {
        let mut io_buf = EpIoBuf::new(ep, data.len());
        io_buf.data_mut()[..data.len()].copy_from_slice(data);

        unsafe { ioctl::ioctl_ep_write(self.fd(), io_buf.header_ptr()) }.map(|v| v as usize)
    }

    /// Read data from a non-control endpoint (`USB_RAW_IOCTL_EP_READ`).
    ///
    /// Uses heap allocation for the I/O buffer (supports up to 64 KB+).
    /// Returns the number of bytes transferred.
    pub fn ep_read(&self, ep: EpHandle, buf: &mut [u8]) -> io::Result<usize> {
        let mut io_buf = EpIoBuf::new(ep, buf.len());

        let transferred =
            unsafe { ioctl::ioctl_ep_read(self.fd(), io_buf.header_ptr_mut()) }? as usize;
        let n = transferred.min(buf.len());
        buf[..n].copy_from_slice(&io_buf.data()[..n]);
        Ok(n)
    }

    /// Switch the gadget to configured state (`USB_RAW_IOCTL_CONFIGURE`).
    pub fn configure(&self) -> io::Result<()> {
        unsafe { ioctl::ioctl_configure(self.fd()) }?;
        Ok(())
    }

    /// Set VBUS power draw (`USB_RAW_IOCTL_VBUS_DRAW`).
    ///
    /// `power` is in 2 mA units.
    pub fn vbus_draw(&self, power: u32) -> io::Result<()> {
        unsafe { ioctl::ioctl_vbus_draw(self.fd(), power) }?;
        Ok(())
    }

    /// Get information about available non-control endpoints (`USB_RAW_IOCTL_EPS_INFO`).
    ///
    /// Returns the endpoint info array and the number of valid entries.
    pub fn eps_info(&self) -> io::Result<(types::UsbRawEpsInfo, usize)> {
        let mut info = UsbRawEpsInfo::default();
        let count = unsafe { ioctl::ioctl_eps_info(self.fd(), &mut info) }?;
        Ok((info, count as usize))
    }

    /// Stall a pending control request on endpoint 0 (`USB_RAW_IOCTL_EP0_STALL`).
    pub fn ep0_stall(&self) -> io::Result<()> {
        unsafe { ioctl::ioctl_ep0_stall(self.fd()) }?;
        Ok(())
    }

    /// Set halt on an endpoint (`USB_RAW_IOCTL_EP_SET_HALT`).
    pub fn ep_set_halt(&self, ep: EpHandle) -> io::Result<()> {
        unsafe { ioctl::ioctl_ep_set_halt(self.fd(), ep.0) }?;
        Ok(())
    }

    /// Clear halt on an endpoint (`USB_RAW_IOCTL_EP_CLEAR_HALT`).
    pub fn ep_clear_halt(&self, ep: EpHandle) -> io::Result<()> {
        unsafe { ioctl::ioctl_ep_clear_halt(self.fd(), ep.0) }?;
        Ok(())
    }

    /// Set wedge on an endpoint (`USB_RAW_IOCTL_EP_SET_WEDGE`).
    pub fn ep_set_wedge(&self, ep: EpHandle) -> io::Result<()> {
        unsafe { ioctl::ioctl_ep_set_wedge(self.fd(), ep.0) }?;
        Ok(())
    }
}

/// Heap-allocated I/O buffer for non-control endpoint operations.
///
/// Uses `Vec<u32>` for 4-byte alignment required by `UsbRawEpIoHeader`.
struct EpIoBuf {
    _buf: Vec<u32>,
    ptr: *mut u8,
    header_size: usize,
}

impl EpIoBuf {
    fn new(ep: EpHandle, data_len: usize) -> Self {
        let header_size = size_of::<UsbRawEpIoHeader>();
        let total = header_size + data_len;
        let u32_count = (total + 3) / 4;
        let mut buf = vec![0u32; u32_count];
        let ptr = buf.as_mut_ptr() as *mut u8;

        unsafe {
            let header = &mut *(ptr as *mut UsbRawEpIoHeader);
            header.ep = ep.0 as u16;
            header.flags = 0;
            header.length = data_len as u32;
        }

        Self {
            _buf: buf,
            ptr,
            header_size,
        }
    }

    fn header_ptr(&self) -> *const UsbRawEpIoHeader {
        self.ptr as *const UsbRawEpIoHeader
    }

    fn header_ptr_mut(&mut self) -> *mut UsbRawEpIoHeader {
        self.ptr as *mut UsbRawEpIoHeader
    }

    fn data(&self) -> &[u8] {
        unsafe {
            let data_ptr = self.ptr.add(self.header_size);
            let header = &*(self.ptr as *const UsbRawEpIoHeader);
            std::slice::from_raw_parts(data_ptr, header.length as usize)
        }
    }

    fn data_mut(&mut self) -> &mut [u8] {
        unsafe {
            let data_ptr = self.ptr.add(self.header_size);
            let header = &*(self.ptr as *const UsbRawEpIoHeader);
            std::slice::from_raw_parts_mut(data_ptr, header.length as usize)
        }
    }
}
