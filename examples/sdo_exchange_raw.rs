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
    let master = RawMaster::new(EthernetSocket::new("eno1")?);

    // set fixed addresses
    master.apwr(0, registers::address::fixed, 1).await.one().unwrap();
    master.apwr(1, registers::address::fixed, 2).await.one().unwrap();

    let slave = 1;

    // initialize mailbox
    let mailbox = Arc::new(Mutex::new(Mailbox::new(master.clone(), 1, 0x1000 .. 0x1103, 0x1104 .. 0x1200).await.unwrap()));
    let mut can = Can::new(mailbox);

    master.fpwr(slave, registers::sii::access, {
        let mut config = registers::SiiAccess::default();
        config.set_owner(registers::SiiOwner::Pdi);
        config
    }).await.one().unwrap();

    // switch to preop
    master.fpwr(slave, registers::al::control, {
        let mut config = registers::AlControlRequest::default();
        config.set_state(registers::AlState::PreOperational.into());
        config.set_ack(true);
        config
    }).await.one().unwrap();

    loop {
        let received = master.fprd(slave, registers::al::response).await.one().unwrap();
        if received.error() {
            let received = master.fprd(slave, registers::al::error).await.one().unwrap();
            panic!("error on state change: {:?}", received);
        }
        if received.state() == registers::AlState::PreOperational.into()  {break}
    }

    let sdo = Sdo::<u16>::complete(0x6041);

    // test read/write
    let received = can.sdo_read(&sdo, u2::new(1)).await.unwrap();
    can.sdo_write(&sdo, u2::new(1), received).await.unwrap();

    Ok(())
}
