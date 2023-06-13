use std::sync::Arc;
use core::time::Duration;
use etherage::{
    PduData, Field, PduAnswer, EthernetSocket, RawMaster, Sdo, SlaveAddress, Slave, CommunicationState,
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
    slave.set_address(1).await;
    slave.init_mailbox().await;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await;
    
    let sdo = Sdo::<u32>::complete(0x1c12);
    
    // test read/write
    println!("read");
    let received = slave.coe().await.sdo_read(&sdo, u2::new(1)).await;
    println!("write");
    slave.coe().await.sdo_write(&sdo, u2::new(1), received).await;
    
//     // test simultaneous read/write
//     let t1 = {
//         let master = master.clone();
//         tokio::task::spawn(async move {
//             let received = master.aprd(reg).await;
//             assert_eq!(received.answers, 1);
//             master.apwr(reg, received.value).await;
//         })
//     };
//     let t2 = {
//         let master = master.clone();
//         tokio::task::spawn(async move {
//             let received = master.aprd(reg).await;
//             assert_eq!(received.answers, 1);
//             master.apwr(reg, received.value).await;
//         })
//     };
//     t1.await.unwrap();
//     t2.await.unwrap();
    
//     println!("received {:x}", value);
    Ok(())
}

