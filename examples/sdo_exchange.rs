use std::sync::Arc;
use core::time::Duration;
use futures_concurrency::future::Join;
use etherage::{
    EthernetSocket, RawMaster, 
    Sdo, SlaveAddress, Slave, CommunicationState,
    };
use bilge::prelude::u2;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let master = Arc::new(RawMaster::new(EthernetSocket::new("eno1")?));
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
    
    let mut slave = Slave::raw(&master, SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await;
    slave.set_address(1).await;
    slave.init_mailbox().await;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await;
    
    let sdo = Sdo::<u32>::complete(0x1c12);
    let priority = u2::new(1);
    
    // test read/write
    let received = slave.coe().await.sdo_read(&sdo, priority).await;
    slave.coe().await.sdo_write(&sdo, priority, received).await;
    
    // test concurrent read/write
    (
        async {
            println!("begin");
            let received = slave.coe().await.sdo_read(&sdo, priority).await;
            println!("between");
            slave.coe().await.sdo_write(&sdo, priority, received).await;
            println!("end");
        },
        async {
            println!("begin");
            let received = slave.coe().await.sdo_read(&sdo, priority).await;
            println!("between");
            slave.coe().await.sdo_write(&sdo, priority, received).await;
            println!("end");
        },
    ).join().await;
    
    Ok(())
}

