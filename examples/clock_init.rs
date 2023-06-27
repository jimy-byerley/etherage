use std::sync::Arc;
use core::time::Duration;
use etherage::{PduData, Field, PduAnswer, EthernetSocket, RawMaster, SyncClock};

pub const socket_name : str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {

    let master = Arc::new(RawMaster::new(EthernetSocket::new(&socket_name)?));
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

    // test read/write
    let mut slave : Vec<Slave> = Vec::new();
    slave.push(Slave::new(&master, SlaveAddress::AutoIncremented(0)));
    slave[0].switch(CommunicationState::Init).await;
    slave[0].set_address(slave.len()).await;
    slave[0].init_mailbox().await;
    slave[0].init_coe().await;
    slave[0].switch(CommunicationState::PreOperational).await;
    slave.push(Slave::new(&master, SlaveAddress::AutoIncremented(1)));
    slave[1].set_address(slave.len()).await;
    slave[1].init_mailbox().await;
    slave[1].init_coe().await;
    slave[1].switch(CommunicationState::PreOperational).await;
    slave.push(Slave::new(&master, SlaveAddress::AutoIncremented(2)));
    slave[2].set_address(slave.len()).await;
    slave[2].init_mailbox().await;
    slave[2].init_coe().await;
    slave[2].switch(CommunicationState::PreOperational).await;

    //Init clock
    let sc = SyncClock::new(master);
    sc.register(slave);
    sc.register(slave);

    Ok(())
}
