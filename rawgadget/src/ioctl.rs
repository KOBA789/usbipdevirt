use std::io;
use std::os::fd::RawFd;

use crate::types::{UsbRawEpIoHeader, UsbRawEpsInfo, UsbRawEventHeader, UsbRawInit};
use crate::usb_types::UsbEndpointDescriptor;

// ioctl direction bits (generic / aarch64)
const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;
const IOC_RDWR: u32 = 3;

const IOC_TYPE: u32 = b'U' as u32;

const fn ioc(dir: u32, nr: u32, size: u32) -> u32 {
    (dir << 30) | (size << 16) | (IOC_TYPE << 8) | nr
}

// ioctl numbers — sizes match the kernel header structs (flexible array headers = 8 bytes)
pub(crate) const IOCTL_INIT: u32 = ioc(IOC_WRITE, 0, 257);
pub(crate) const IOCTL_RUN: u32 = ioc(IOC_NONE, 1, 0);
pub(crate) const IOCTL_EVENT_FETCH: u32 = ioc(IOC_READ, 2, 8);
pub(crate) const IOCTL_EP0_WRITE: u32 = ioc(IOC_WRITE, 3, 8);
pub(crate) const IOCTL_EP0_READ: u32 = ioc(IOC_RDWR, 4, 8);
pub(crate) const IOCTL_EP_ENABLE: u32 = ioc(IOC_WRITE, 5, 9);
pub(crate) const IOCTL_EP_DISABLE: u32 = ioc(IOC_WRITE, 6, 4);
pub(crate) const IOCTL_EP_WRITE: u32 = ioc(IOC_WRITE, 7, 8);
pub(crate) const IOCTL_EP_READ: u32 = ioc(IOC_RDWR, 8, 8);
pub(crate) const IOCTL_CONFIGURE: u32 = ioc(IOC_NONE, 9, 0);
pub(crate) const IOCTL_VBUS_DRAW: u32 = ioc(IOC_WRITE, 10, 4);
pub(crate) const IOCTL_EPS_INFO: u32 = ioc(IOC_READ, 11, 960);
pub(crate) const IOCTL_EP0_STALL: u32 = ioc(IOC_NONE, 12, 0);
pub(crate) const IOCTL_EP_SET_HALT: u32 = ioc(IOC_WRITE, 13, 4);
pub(crate) const IOCTL_EP_CLEAR_HALT: u32 = ioc(IOC_WRITE, 14, 4);
pub(crate) const IOCTL_EP_SET_WEDGE: u32 = ioc(IOC_WRITE, 15, 4);

/// Call `libc::ioctl` and convert the result to `io::Result`.
unsafe fn raw_ioctl(fd: RawFd, request: u32, arg: libc::c_ulong) -> io::Result<i32> {
    let ret = unsafe { libc::ioctl(fd, request as libc::c_ulong, arg) };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

// --- Pointer-based ioctls ---

pub(crate) unsafe fn ioctl_init(fd: RawFd, arg: *const UsbRawInit) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_INIT, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_event_fetch(
    fd: RawFd,
    arg: *mut UsbRawEventHeader,
) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EVENT_FETCH, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep0_write(
    fd: RawFd,
    arg: *const UsbRawEpIoHeader,
) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP0_WRITE, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep0_read(
    fd: RawFd,
    arg: *mut UsbRawEpIoHeader,
) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP0_READ, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep_enable(
    fd: RawFd,
    arg: *const UsbEndpointDescriptor,
) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_ENABLE, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep_write(
    fd: RawFd,
    arg: *const UsbRawEpIoHeader,
) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_WRITE, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep_read(
    fd: RawFd,
    arg: *mut UsbRawEpIoHeader,
) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_READ, arg as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_eps_info(fd: RawFd, arg: *mut UsbRawEpsInfo) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EPS_INFO, arg as libc::c_ulong) }
}

// --- Value-based ioctls ---

pub(crate) unsafe fn ioctl_run(fd: RawFd) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_RUN, 0) }
}

pub(crate) unsafe fn ioctl_configure(fd: RawFd) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_CONFIGURE, 0) }
}

pub(crate) unsafe fn ioctl_vbus_draw(fd: RawFd, power: u32) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_VBUS_DRAW, power as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep0_stall(fd: RawFd) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP0_STALL, 0) }
}

pub(crate) unsafe fn ioctl_ep_disable(fd: RawFd, ep: u32) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_DISABLE, ep as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep_set_halt(fd: RawFd, ep: u32) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_SET_HALT, ep as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep_clear_halt(fd: RawFd, ep: u32) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_CLEAR_HALT, ep as libc::c_ulong) }
}

pub(crate) unsafe fn ioctl_ep_set_wedge(fd: RawFd, ep: u32) -> io::Result<i32> {
    unsafe { raw_ioctl(fd, IOCTL_EP_SET_WEDGE, ep as libc::c_ulong) }
}
