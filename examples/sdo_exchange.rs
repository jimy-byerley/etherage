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
    
    let slave = 1;
    
//     let received = master.fprd(slave, registers::al::response).await.one();
//     if received.error() {
//         let received = master.fprd(slave, registers::al::error).await.one();
//         panic!("error on before change: {:?}", received);
//     }
//     
//     // configure sync manager
//     master.fpwr(slave, registers::sync_manager::interface.mailbox_write(), {
//         let mut config = registers::SyncManagerChannel::default();
//         config.set_address(registers::mailbox_buffers[0].byte as u16);
//         config.set_length(registers::mailbox_buffers[0].len as u16);
// //         config.set_address(0x1000);
// //         config.set_length(0x80);
//         config.set_mode(registers::SyncMode::Mailbox);
//         config.set_direction(registers::SyncDirection::Write);
//         config.set_dls_user_event(true);
//         config.set_ec_event(true);
//         config.set_enable(true);
//         config
//     }).await.one();
//     
//     
//     master.fpwr(slave, registers::sync_manager::interface.mailbox_read(), {
//         let mut config = registers::SyncManagerChannel::default();
//         config.set_address(registers::mailbox_buffers[1].byte as u16);
//         config.set_length(registers::mailbox_buffers[1].len as u16);
// //         config.set_address(0x1080);
// //         config.set_length(0x80);
//         config.set_mode(registers::SyncMode::Mailbox);
//         config.set_direction(registers::SyncDirection::Read);
//         config.set_dls_user_event(true);
//         config.set_ec_event(true);
//         config.set_enable(true);
//         config
//     }).await.one();


    // initialize mailbox
    let mut mailbox = Mailbox::new(&master, 1, 0x1000 .. 0x1103, 0x1104 .. 0x1200).await;
    let mut can = Can::new(&mut mailbox);
        
    master.fpwr(slave, registers::sii::access, {
        let mut config = registers::SiiAccess::default();
        config.set_owner(registers::SiiOwner::Pdi);
        config
    }).await.one();
    
    // switch to preop
    master.fpwr(slave, registers::al::control, {
        let mut config = registers::AlControlRequest::default();
        config.set_state(registers::AlState::PreOperational);
        config.set_ack(true);
        config
    }).await.one();
    
    loop {
        let received = master.fprd(slave, registers::al::response).await;
        assert_eq!(received.answers, 1);
//         assert_eq!(received.value.error(), false);
        if received.value.error() {
            let received = master.fprd(slave, registers::al::error).await;
            assert_eq!(received.answers, 1);
            panic!("error on state change: {:?}", received.value);
        }
        if received.value.state() == registers::AlState::PreOperational  {break}
    }
    
    let sdo = Sdo::<u16>::complete(0x6041);
    
    // test read/write
    println!("read");
    let received = can.sdo_read(&sdo, u2::new(1)).await;
    println!("write");
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

