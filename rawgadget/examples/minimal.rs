use rawgadget::{Event, RawGadgetDevice, UsbSpeed};

fn ep_name(name: &[u8; 16]) -> &str {
    let end = name.iter().position(|&b| b == 0).unwrap_or(16);
    std::str::from_utf8(&name[..end]).unwrap_or("???")
}

fn main() -> std::io::Result<()> {
    let dev = RawGadgetDevice::open()?;
    dev.init("1000480000.usb", "1000480000.usb", UsbSpeed::High)?;
    dev.run()?;

    println!("Waiting for events...");
    loop {
        let event = dev.event_fetch()?;
        match event {
            Event::Connect => {
                println!("event: CONNECT");
                let (info, count) = dev.eps_info()?;
                for i in 0..count {
                    let ep = &info.eps[i];
                    println!(
                        "  ep #{}: name={} addr={} type=[{}{}{}] dir=[{}{}] maxpacket={}",
                        i,
                        ep_name(&ep.name),
                        ep.addr,
                        if ep.caps.type_iso() { "iso " } else { "" },
                        if ep.caps.type_bulk() { "blk " } else { "" },
                        if ep.caps.type_int() { "int " } else { "" },
                        if ep.caps.dir_in() { "in " } else { "" },
                        if ep.caps.dir_out() { "out " } else { "" },
                        ep.limits.maxpacket_limit,
                    );
                }
            }
            Event::Control(ctrl) => {
                println!(
                    "event: CONTROL bRequestType=0x{:02x} bRequest=0x{:02x} \
                     wValue=0x{:04x} wIndex=0x{:04x} wLength={}",
                    ctrl.bRequestType,
                    ctrl.bRequest,
                    { ctrl.wValue },
                    { ctrl.wIndex },
                    { ctrl.wLength },
                );
                dev.ep0_stall()?;
            }
            Event::Suspend => println!("event: SUSPEND"),
            Event::Resume => println!("event: RESUME"),
            Event::Reset => println!("event: RESET"),
            Event::Disconnect => println!("event: DISCONNECT"),
            Event::Unknown(t) => println!("event: UNKNOWN({})", t),
        }
    }
}
