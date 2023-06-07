use std::sync::Arc;
use core::time::Duration;
use etherage::{
    PduData, Field, PduAnswer, EthernetSocket, RawMaster, Sdo,
    mailbox::Mailbox,
    can::Can,
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
    
    // set fixed addresses
    master.apwr(0, registers::address::fixed, 1).await;
    master.apwr(1, registers::address::fixed, 2).await;
    
//     // configure sync manager
//     let mut config = registers::SyncManagerChannel::default();
//     config.set_address(registers::mailbox_buffers[0].byte as u16);
//     config.set_length(registers::mailbox_buffers[0].len as u16);
//     config.set_buffer_type(registers::SyncBufferType::Mailbox);
//     config.set_direction(registers::SyncBufferDirection::Write);
//     config.set_enable(true);
//     assert_eq!(master.fpwr(1, registers::sync_manager::interface.mailbox_read(), config).await.answers, 1);
//     
//     let mut config = registers::SyncManagerChannel::default();
//     config.set_address(registers::mailbox_buffers[0].byte as u16);
//     config.set_length(registers::mailbox_buffers[0].len as u16);
//     config.set_buffer_type(registers::SyncBufferType::Mailbox);
//     config.set_direction(registers::SyncBufferDirection::Read);
//     config.set_enable(true);
//     assert_eq!(master.fpwr(1, registers::sync_manager::interface.mailbox_write(), config).await.answers, 1);
//     
    // initialize mailbox
    let mut mailbox = Mailbox::new(&master, 1);
    let mut can = Can::new(&mut mailbox);
    
    let sdo = Sdo::<u16>::complete(0x6041);
    
    // test read/write
    let received = can.sdo_read(&sdo, u2::new(1)).await;
    can.sdo_write(&sdo, u2::new(1), received).await;
    
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

