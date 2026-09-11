//! List ADB-capable USB devices without opening them.
//!
//! ```sh
//! cargo run --example devices
//! ```

#[tokio::main]
async fn main() -> rsadb::Result<()> {
    let devices = rsadb::transport::usb::list().await?;
    if devices.is_empty() {
        println!("no ADB USB devices attached");
    }
    for d in devices {
        println!(
            "{:<24} {:04x}:{:04x} {} {} (interface {})",
            d.serial().unwrap_or("(no serial)"),
            d.vendor_id(),
            d.product_id(),
            d.manufacturer().unwrap_or(""),
            d.product().unwrap_or(""),
            d.interface_number()
        );
    }
    Ok(())
}
