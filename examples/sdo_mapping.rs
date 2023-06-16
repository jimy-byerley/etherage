use std::sync::Arc;
use core::time::Duration;
use etherage::{
    PduData, Field, PduAnswer, EthernetSocket, RawMaster, 
    Sdo, SlaveAddress, Slave, CommunicationState,
    Mapping, Group, 
    registers,
    mapping,
    sdo,
    mapping::SyncDirection,
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
    
    println!("create mapping");
    let config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slave = mapping.slave(1);
        let status = slave.register(registers::al::status);
        let mut channel = slave.channel(2, SyncDirection::Read, sdo::SyncChannel{ index: 0x1c12, num: 4 });
            let mut pdo = channel.push(sdo::Pdo{ index: 0x1600, num: 10 });
                let status = pdo.push(Sdo::<u16>::complete(0x6041));
                let error = pdo.push(Sdo::<u16>::complete(0x603f));
                let position = pdo.push(Sdo::<i32>::complete(0x6064));
    println!("done");
    
    let mut allocator = mapping::Allocator::new(&master);
    let mut group = allocator.group(mapping);
    
    let mut slave = Slave::new(&master, SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await;
    slave.set_address(1).await;
    slave.init_mailbox().await;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await;
    group.configure(&slave).await;
    slave.switch(CommunicationState::SafeOperational).await;
    slave.switch(CommunicationState::Operational).await;
    
    for _ in 0 .. 20 {
        group.exchange();
        println!("received {}  {}  {}", group.get(status), group.get(error), group.get(position));
    }
    
//     let sdo = Sdo::<u32>::complete(0x1c12);
//     
//     // test read/write
//     let received = slave.coe().await.sdo_read(&sdo, u2::new(1)).await;
//     slave.coe().await.sdo_write(&sdo, u2::new(1), received).await;
    
    Ok(())
}

