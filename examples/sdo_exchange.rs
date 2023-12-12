use std::error::Error;
use futures_concurrency::future::Join;
use etherage::{
    EthernetSocket, RawMaster,
    Sdo, SlaveAddress, Slave, CommunicationState,
    };
use bilge::prelude::u2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Init raw master");
    let master = RawMaster::new(EthernetSocket::new("eno1")?);
    let mut slave = Slave::raw(master.clone(), SlaveAddress::AutoIncremented(0));
    println!("Switch init");
    slave.switch(CommunicationState::Init).await.unwrap();
    println!("Set address");
    slave.set_address(1).await.unwrap();
    println!("Set mailbox");
    slave.init_mailbox().await.unwrap();
    println!("Set coe");
    slave.init_coe().await;

    println!("Switch to preop");
    slave.switch(CommunicationState::PreOperational).await.unwrap();

    let sdo = Sdo::<u32>::complete(0x1c12);
    let priority = u2::new(1);

    // test read/write
    println!("Test read and write");
    let received = slave.coe().await.sdo_read(&sdo, priority).await.unwrap();
    slave.coe().await.sdo_write(&sdo, priority, received).await.unwrap();

    // test concurrent read/write
    (
        async {
            println!("SDO read - 1 - Begin");
            let received = slave.coe().await.sdo_read(&sdo, priority).await.unwrap();
            slave.coe().await.sdo_write(&sdo, priority, received).await.unwrap();
            println!("SDO read - 1 - End");
        },
        async {
            println!("SDO read - 2 - Begin");
            let received = slave.coe().await.sdo_read(&sdo, priority).await.unwrap();
            slave.coe().await.sdo_write(&sdo, priority, received).await.unwrap();
            println!("SDO read - 2 - End");
        },
    ).join().await;

    Ok(())
}
