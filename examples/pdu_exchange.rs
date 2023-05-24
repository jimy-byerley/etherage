use etherage::{PduData, EthernetSocket, RawMaster};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut master = RawMaster::new(EthernetSocket::new("eno1")?);
    std::thread::spawn(|| loop {
        master.receive();
    });
    let some: u16 = master.aprd(0x1234).await;
    Ok(())
}
