use std::sync::Arc;
use core::time::Duration;
use etherage::{
    PduData, Field, PduAnswer, EthernetSocket, RawMaster, 
    Sdo, SlaveAddress, Slave, CommunicationState,
    registers,
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
    
    let mut slave = Slave::new(&master, SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await;
    slave.set_address(1).await;
    slave.init_mailbox().await;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await;
    
    let sdo = Sdo::<u32>::complete(0x1c12);
    
    // test read/write
    let received = slave.coe().await.sdo_read(&sdo, u2::new(1)).await;
    slave.coe().await.sdo_write(&sdo, u2::new(1), received).await;
    
    // test concurrent read/write
    futures::join!(
        async {
            println!("begin");
            let received = slave.coe().await.sdo_read(&sdo, u2::new(1)).await;
            println!("between");
            slave.coe().await.sdo_write(&sdo, u2::new(1), received).await;
            println!("end");
        },
        async {
            println!("begin");
            let received = slave.coe().await.sdo_read(&sdo, u2::new(1)).await;
            println!("between");
            slave.coe().await.sdo_write(&sdo, u2::new(1), received).await;
            println!("end");
        },
    );
    
    Ok(())
}

