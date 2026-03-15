use usbip::{Direction, UrbResponse};

fn main() {
    // List devices to find busid
    let devices = usbip::list_devices("localhost:3240").unwrap();
    if devices.is_empty() {
        eprintln!("No devices found");
        return;
    }

    let busid = &devices[0].busid;
    println!("Importing device: {busid}");

    // Import
    let (dev, mut conn) = usbip::import_device("localhost:3240", busid).unwrap();
    println!(
        "Imported: idVendor=0x{:04x}, idProduct=0x{:04x}",
        dev.id_vendor, dev.id_product
    );

    // GET_DESCRIPTOR (Device), 18 bytes
    let setup = [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 18, 0x00];
    let seqnum = conn
        .send_submit(0, Direction::In, 0, 18, setup, &[], 0)
        .unwrap();
    println!("Sent GET_DESCRIPTOR, seqnum={seqnum}");

    // Receive response
    match conn.recv().unwrap() {
        UrbResponse::Submit(ret) => {
            println!(
                "RET_SUBMIT: seqnum={}, status={}, actual_length={}",
                ret.seqnum, ret.status, ret.actual_length
            );
            if !ret.data.is_empty() {
                println!("Device descriptor ({} bytes):", ret.data.len());
                for (i, b) in ret.data.iter().enumerate() {
                    print!("{b:02x} ");
                    if (i + 1) % 16 == 0 {
                        println!();
                    }
                }
                println!();
            }
        }
        UrbResponse::Unlink(ret) => {
            println!(
                "Unexpected RET_UNLINK: seqnum={}, status={}",
                ret.seqnum, ret.status
            );
        }
    }

    // Shutdown
    conn.stream().shutdown(std::net::Shutdown::Both).ok();
}
