use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use clap::Parser;
use log::{debug, error, info, warn};
use rawgadget::usb_types::{
    USB_DT_ENDPOINT, USB_REQ_GET_DESCRIPTOR, USB_REQ_SET_ADDRESS, USB_REQ_SET_CONFIGURATION,
};
use rawgadget::{Event, RawGadgetDevice, UsbEndpointDescriptor, UsbSpeed};
use usbip::{Direction, RetSubmit, UrbResponse, import_device, list_devices};

#[derive(Parser)]
#[command(about = "Bridge USB/IP devices to raw-gadget")]
struct Args {
    /// USB/IP server host
    #[arg(long, default_value = "localhost")]
    host: String,

    /// USB/IP server port
    #[arg(long, default_value_t = 3240)]
    port: u16,

    /// UDC driver/device name
    #[arg(long, default_value = "1000480000.usb")]
    udc: String,
}

// --- USB/IP Bridge ---

struct UsbipBridge {
    writer: Mutex<usbip::UsbipWriter>,
    pending: Mutex<HashMap<u32, mpsc::SyncSender<io::Result<RetSubmit>>>>,
}

impl UsbipBridge {
    fn new(writer: usbip::UsbipWriter) -> Self {
        Self {
            writer: Mutex::new(writer),
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn submit(
        &self,
        ep: u32,
        direction: Direction,
        setup: [u8; 8],
        data: &[u8],
        transfer_buffer_length: u32,
    ) -> io::Result<RetSubmit> {
        let (tx, rx) = mpsc::sync_channel(1);

        {
            let mut writer = self.writer.lock().unwrap();

            let seqnum = writer.send_submit(
                ep,
                direction,
                0, // transfer_flags
                transfer_buffer_length,
                setup,
                data,
                0, // interval
            )?;

            self.pending.lock().unwrap().insert(seqnum, tx);
        }

        rx.recv().unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "reader thread disconnected",
            ))
        })
    }

    fn reader_thread(bridge: Arc<UsbipBridge>, mut reader: usbip::UsbipReader) {
        let result = (|| -> io::Result<()> {
            loop {
                match reader.recv()? {
                    UrbResponse::Submit(ret) => {
                        let tx = {
                            let mut pending = bridge.pending.lock().unwrap();
                            pending.remove(&ret.seqnum)
                        };

                        if let Some(tx) = tx {
                            let _ = tx.send(Ok(ret));
                        } else {
                            warn!("RET_SUBMIT for unknown seqnum {}", ret.seqnum);
                        }
                    }
                    UrbResponse::Unlink(ret) => {
                        warn!("received RET_UNLINK seqnum={}", ret.seqnum);
                    }
                }
            }
        })();

        if let Err(e) = result {
            error!("reader thread error: {e}");
        }

        // Drain all pending requests with error
        let mut pending = bridge.pending.lock().unwrap();
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "reader thread terminated",
            )));
        }
    }
}

// --- Config descriptor parsing ---

fn parse_endpoint_descriptors(config_data: &[u8]) -> Vec<UsbEndpointDescriptor> {
    let mut endpoints = Vec::new();
    let mut offset = 0;

    while offset + 1 < config_data.len() {
        let b_length = config_data[offset] as usize;
        if b_length < 2 || offset + b_length > config_data.len() {
            break;
        }
        let b_descriptor_type = config_data[offset + 1];

        if b_descriptor_type == USB_DT_ENDPOINT && b_length >= 7 {
            endpoints.push(UsbEndpointDescriptor {
                bLength: config_data[offset] as u8,
                bDescriptorType: b_descriptor_type,
                bEndpointAddress: config_data[offset + 2],
                bmAttributes: config_data[offset + 3],
                wMaxPacketSize: u16::from_le_bytes([
                    config_data[offset + 4],
                    config_data[offset + 5],
                ]),
                bInterval: config_data[offset + 6],
                bRefresh: if b_length > 7 { config_data[offset + 7] } else { 0 },
                bSynchAddress: if b_length > 8 { config_data[offset + 8] } else { 0 },
            });
        }

        offset += b_length;
    }

    endpoints
}

// --- Speed mapping ---

fn map_speed(speed: u32) -> UsbSpeed {
    match speed {
        1 => UsbSpeed::Low,
        2 => UsbSpeed::Full,
        3 => UsbSpeed::High,
        4 => UsbSpeed::Wireless,
        5 => UsbSpeed::Super,
        6 => UsbSpeed::SuperPlus,
        _ => UsbSpeed::Unknown,
    }
}

// --- Endpoint data threads ---

fn spawn_ep_threads(
    gadget: &Arc<RawGadgetDevice>,
    bridge: &Arc<UsbipBridge>,
    ep_descs: &[UsbEndpointDescriptor],
    ep_handles: &[rawgadget::EpHandle],
) -> Vec<thread::JoinHandle<()>> {
    let mut handles = Vec::new();

    for (desc, &ep_handle) in ep_descs.iter().zip(ep_handles.iter()) {
        let ep_addr = desc.bEndpointAddress;
        let ep_num = (ep_addr & 0x0f) as u32;
        let is_in = ep_addr & 0x80 != 0;
        let max_packet_size = desc.wMaxPacketSize as u32;

        let gadget = Arc::clone(gadget);
        let bridge = Arc::clone(bridge);

        if is_in {
            // IN endpoint: device→host
            // Read from USB/IP, write to raw-gadget
            let handle = thread::spawn(move || {
                debug!("EP{ep_num} IN thread started");
                loop {
                    let resp =
                        match bridge.submit(ep_num, Direction::In, [0; 8], &[], max_packet_size) {
                            Ok(r) => r,
                            Err(e) => {
                                error!("EP{ep_num} IN submit error: {e}");
                                break;
                            }
                        };
                    if resp.status != 0 {
                        warn!("EP{ep_num} IN submit status: {}", resp.status);
                        break;
                    }
                    if let Err(e) = gadget.ep_write(ep_handle, &resp.data) {
                        error!("EP{ep_num} IN ep_write error: {e}");
                        break;
                    }
                }
                debug!("EP{ep_num} IN thread exiting");
            });
            handles.push(handle);
        } else {
            // OUT endpoint: host→device
            // Read from raw-gadget, write to USB/IP
            let handle = thread::spawn(move || {
                debug!("EP{ep_num} OUT thread started");
                let mut buf = vec![0u8; max_packet_size as usize];
                loop {
                    let n = match gadget.ep_read(ep_handle, &mut buf) {
                        Ok(n) => n,
                        Err(e) => {
                            error!("EP{ep_num} OUT ep_read error: {e}");
                            break;
                        }
                    };
                    match bridge.submit(ep_num, Direction::Out, [0; 8], &buf[..n], n as u32) {
                        Ok(resp) => {
                            if resp.status != 0 {
                                warn!("EP{ep_num} OUT submit status: {}", resp.status);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("EP{ep_num} OUT submit error: {e}");
                            break;
                        }
                    }
                }
                debug!("EP{ep_num} OUT thread exiting");
            });
            handles.push(handle);
        }
    }

    handles
}

// --- Main ---

fn main() -> io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let server_addr = format!("{}:{}", args.host, args.port);

    // Step 1: List devices and pick the first one
    info!("Listing devices on {server_addr}...");
    let devices = list_devices(&server_addr)?;
    if devices.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no devices available on USB/IP server",
        ));
    }
    let busid = devices[0].busid.clone();
    info!(
        "Found device: busid={busid} {:04x}:{:04x}",
        devices[0].id_vendor, devices[0].id_product
    );

    // Step 2: Import the device
    info!("Importing device {busid}...");
    let (dev_info, conn) = import_device(&server_addr, &busid)?;
    info!(
        "Imported: {:04x}:{:04x} speed={}",
        dev_info.id_vendor, dev_info.id_product, dev_info.speed
    );

    // Step 3: Set up UsbipBridge with split connection
    let (reader, writer) = conn.into_split()?;
    let bridge = Arc::new(UsbipBridge::new(writer));

    // Spawn reader thread
    {
        let bridge = Arc::clone(&bridge);
        thread::spawn(move || UsbipBridge::reader_thread(bridge, reader));
    }

    // Step 4: Initialize raw-gadget
    let gadget = Arc::new(RawGadgetDevice::open()?);
    let udc_name = &args.udc;
    gadget.init(udc_name, udc_name, map_speed(dev_info.speed))?;
    info!("raw-gadget initialized (UDC: {udc_name})");
    gadget.run()?;
    info!("raw-gadget running");

    // Step 5: Event loop
    let mut cached_config_desc: Option<Vec<u8>> = None;
    let mut ep_threads: Vec<thread::JoinHandle<()>> = Vec::new();

    loop {
        let event = gadget.event_fetch()?;
        match event {
            Event::Connect => {
                info!("event: Connect");
            }
            Event::Control(ctrl) => {
                let b_request_type = ctrl.bRequestType;
                let b_request = ctrl.bRequest;
                let w_value = ctrl.wValue;
                let w_index = ctrl.wIndex;
                let w_length = ctrl.wLength;

                debug!(
                    "event: Control bRequestType=0x{b_request_type:02x} bRequest=0x{b_request:02x} \
                     wValue=0x{w_value:04x} wIndex=0x{w_index:04x} wLength={w_length}"
                );

                if b_request == USB_REQ_SET_ADDRESS {
                    // SET_ADDRESS: don't forward to USB/IP, just ACK locally
                    gadget.ep0_read(&mut [])?;
                } else if b_request == USB_REQ_SET_CONFIGURATION
                    && (b_request_type & 0x80) == 0
                    && w_length == 0
                {
                    handle_set_configuration(
                        &gadget,
                        &bridge,
                        &ctrl,
                        &cached_config_desc,
                        &mut ep_threads,
                    )?;
                } else if (b_request_type & 0x80) != 0 {
                    // IN control transfer
                    handle_control_in(&gadget, &bridge, &ctrl, &mut cached_config_desc)?;
                } else if w_length > 0 {
                    // OUT control transfer with data
                    handle_control_out_with_data(&gadget, &bridge, &ctrl)?;
                } else {
                    // OUT control transfer without data
                    handle_control_out_no_data(&gadget, &bridge, &ctrl)?;
                }
            }
            Event::Disconnect | Event::Reset => {
                info!("event: {}", if matches!(event, Event::Disconnect) { "Disconnect" } else { "Reset" });
                for h in ep_threads.drain(..) {
                    let _ = h.join();
                }
                cached_config_desc = None;
                if matches!(event, Event::Disconnect) {
                    debug!("all EP threads joined after disconnect");
                }
            }
            Event::Suspend => {
                info!("event: Suspend");
            }
            Event::Resume => {
                info!("event: Resume");
            }
            Event::Unknown(ty) => {
                warn!("event: Unknown({ty})");
            }
        }
    }
}

fn ctrl_to_setup(ctrl: &rawgadget::UsbCtrlRequest) -> [u8; 8] {
    let mut setup = [0u8; 8];
    setup[0] = ctrl.bRequestType;
    setup[1] = ctrl.bRequest;
    setup[2..4].copy_from_slice(&ctrl.wValue.to_le_bytes());
    setup[4..6].copy_from_slice(&ctrl.wIndex.to_le_bytes());
    setup[6..8].copy_from_slice(&ctrl.wLength.to_le_bytes());
    setup
}

fn handle_control_in(
    gadget: &RawGadgetDevice,
    bridge: &UsbipBridge,
    ctrl: &rawgadget::UsbCtrlRequest,
    cached_config_desc: &mut Option<Vec<u8>>,
) -> io::Result<()> {
    let setup = ctrl_to_setup(ctrl);
    let w_length = ctrl.wLength;

    let resp = bridge.submit(0, Direction::In, setup, &[], w_length as u32)?;
    if resp.status == 0 {
        // Cache configuration descriptor
        if ctrl.bRequest == USB_REQ_GET_DESCRIPTOR && (ctrl.wValue >> 8) == 0x02 {
            *cached_config_desc = Some(resp.data.clone());
            debug!("cached config descriptor ({} bytes)", resp.data.len());
        }
        gadget.ep0_write(&resp.data)?;
    } else {
        warn!("control IN stall (status={})", resp.status);
        gadget.ep0_stall()?;
    }
    Ok(())
}

fn handle_control_out_with_data(
    gadget: &RawGadgetDevice,
    bridge: &UsbipBridge,
    ctrl: &rawgadget::UsbCtrlRequest,
) -> io::Result<()> {
    let setup = ctrl_to_setup(ctrl);
    let w_length = ctrl.wLength;

    // Read data from host
    let mut buf = vec![0u8; w_length as usize];
    let n = gadget.ep0_read(&mut buf)?;
    let data = &buf[..n];

    // ep0_read already completed the status stage on the USB bus,
    // so we can't stall retroactively. Just forward and log errors.
    let resp = bridge.submit(0, Direction::Out, setup, data, w_length as u32)?;
    if resp.status != 0 {
        warn!("control OUT (with data) remote status: {}", resp.status);
    }
    Ok(())
}

fn handle_control_out_no_data(
    gadget: &RawGadgetDevice,
    bridge: &UsbipBridge,
    ctrl: &rawgadget::UsbCtrlRequest,
) -> io::Result<()> {
    let setup = ctrl_to_setup(ctrl);

    let resp = bridge.submit(0, Direction::Out, setup, &[], 0)?;
    if resp.status == 0 {
        gadget.ep0_read(&mut [])?;
    } else {
        warn!("control OUT (no data) stall (status={})", resp.status);
        gadget.ep0_stall()?;
    }
    Ok(())
}

fn handle_set_configuration(
    gadget: &Arc<RawGadgetDevice>,
    bridge: &Arc<UsbipBridge>,
    ctrl: &rawgadget::UsbCtrlRequest,
    cached_config_desc: &Option<Vec<u8>>,
    ep_threads: &mut Vec<thread::JoinHandle<()>>,
) -> io::Result<()> {
    let setup = ctrl_to_setup(ctrl);

    // Forward SET_CONFIGURATION to USB/IP
    let resp = bridge.submit(0, Direction::Out, setup, &[], 0)?;
    if resp.status != 0 {
        warn!("SET_CONFIGURATION failed on USB/IP side (status={})", resp.status);
        gadget.ep0_stall()?;
        return Ok(());
    }

    // Parse endpoints from cached config descriptor
    let config_data = cached_config_desc.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "SET_CONFIGURATION without cached config descriptor",
        )
    })?;

    let ep_descs = parse_endpoint_descriptors(config_data);
    info!("SET_CONFIGURATION: found {} endpoints", ep_descs.len());

    // Enable endpoints
    let mut ep_handles = Vec::new();
    for desc in &ep_descs {
        let ep_addr = desc.bEndpointAddress;
        let handle = gadget.ep_enable(desc)?;
        debug!(
            "enabled EP 0x{ep_addr:02x} ({})",
            if ep_addr & 0x80 != 0 { "IN" } else { "OUT" }
        );
        ep_handles.push(handle);
    }

    // vbus_draw: bMaxPower is at offset 7 in config descriptor, in 2mA units
    if config_data.len() > 7 {
        let max_power = config_data[7] as u32;
        gadget.vbus_draw(max_power)?;
        debug!("vbus_draw({max_power})");
    }

    // Mark as configured
    gadget.configure()?;
    info!("gadget configured");

    // ACK the SET_CONFIGURATION before spawning EP threads
    gadget.ep0_read(&mut [])?;

    // Spawn EP data threads
    *ep_threads = spawn_ep_threads(gadget, bridge, &ep_descs, &ep_handles);

    Ok(())
}
