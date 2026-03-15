fn main() {
    let devices = usbip::list_devices("localhost:3240").unwrap();
    println!("Found {} device(s):", devices.len());

    for dev in &devices {
        println!("  busid: {}", dev.busid);
        println!("    path: {}", dev.path);
        println!(
            "    busnum={}, devnum={}, speed={}",
            dev.busnum, dev.devnum, dev.speed
        );
        println!(
            "    idVendor=0x{:04x}, idProduct=0x{:04x}, bcdDevice=0x{:04x}",
            dev.id_vendor, dev.id_product, dev.bcd_device
        );
        println!(
            "    class={}, subclass={}, protocol={}",
            dev.device_class, dev.device_sub_class, dev.device_protocol
        );
        println!(
            "    configurations={}, interfaces={}",
            dev.num_configurations, dev.num_interfaces
        );
        for (i, iface) in dev.interfaces.iter().enumerate() {
            println!(
                "      interface {i}: class={}, subclass={}, protocol={}",
                iface.class, iface.sub_class, iface.protocol
            );
        }
    }
}
