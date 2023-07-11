use std::sync::Arc;
use core::time::Duration;
use etherage::{
    EthernetSocket, RawMaster, Sdo,
    mailbox::Mailbox,
    can::Can,
    registers,
    };
use tokio::sync::Mutex;
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

    // initialize mailbox
    let mailbox = Arc::new(Mutex::new(Mailbox::new(&master, 1, 0x1000 .. 0x1103, 0x1104 .. 0x1200).await));
    let mut can = Can::new(mailbox);
    
    master.fpwr(slave, registers::sii::access, {
        let mut config = registers::SiiAccess::default();
        config.set_owner(registers::SiiOwner::Pdi);
        config
    }).await.one();
    
    // switch to preop
    master.fpwr(slave, registers::al::control, {
        let mut config = registers::AlControlRequest::default();
        config.set_state(registers::AlState::PreOperational.into());
        config.set_ack(true);
        config
    }).await.one();
    
    loop {
        let received = master.fprd(slave, registers::al::response).await;
        assert_eq!(received.answers, 1);
        if received.value.error() {
            let received = master.fprd(slave, registers::al::error).await;
            assert_eq!(received.answers, 1);
            panic!("error on state change: {:?}", received.value);
        }
        if received.value.state() == registers::AlState::PreOperational.into()  {break}
    }
    
    let sdo = Sdo::<u16>::complete(0x6041);
    
    // test read/write
    let received = can.sdo_read(&sdo, u2::new(1)).await;
    can.sdo_write(&sdo, u2::new(1), received).await;
    
    Ok(())
}

