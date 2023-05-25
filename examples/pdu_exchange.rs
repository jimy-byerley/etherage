use std::sync::Arc;
use core::time::Duration;
use etherage::{PduData, EthernetSocket, RawMaster};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut master = Arc::new(RawMaster::new(EthernetSocket::new("eno1")?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.receive();
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.send();
    })};
    std::thread::sleep(Duration::from_millis(500));
    let some: u16 = master.aprd(0x1234).await;
    Ok(())
}
