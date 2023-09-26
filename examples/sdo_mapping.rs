use std::{
    sync::Arc,
    error::Error,
    };
use core::time::Duration;
use etherage::{
    EthernetSocket, RawMaster,
    Slave, SlaveAddress, CommunicationState,
    sdo::{self, Sdo, SyncDirection},
    mapping::{self, Mapping},
    registers,
    };

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = Arc::new(RawMaster::new(EthernetSocket::new("eno1")?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.receive().unwrap();
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.send().unwrap();
    })};
    std::thread::sleep(Duration::from_millis(500));

    println!("create mapping");
    let config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slave = mapping.slave(1);
        let restatus = slave.register(SyncDirection::Read, registers::al::status);
        let mut channel = slave.channel(sdo::SyncChannel{ index: 0x1c12, direction: SyncDirection::Write, capacity: 10 });
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1600, false, 10));
                let control = pdo.push(Sdo::<u16>::complete(0x6040));
        let mut channel = slave.channel(sdo::SyncChannel{ index: 0x1c13, direction: SyncDirection::Read, capacity: 10 });
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1a00, false, 10));
                let status = pdo.push(Sdo::<u16>::complete(0x6041));
                let error = pdo.push(Sdo::<u16>::complete(0x603f));
                let position = pdo.push(Sdo::<i32>::complete(0x6064));
                let torque = pdo.push(Sdo::<i16>::complete(0x6077));
    drop(slave);
    println!("done {:#?}", config);

    let allocator = mapping::Allocator::new();
    let group = allocator.group(&master, &mapping);

    println!("group {:#?}", group);
    println!("fields  {:#?}", (control, status, error, position));

    let mut slave = Slave::raw(master.clone(), SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await?;
    slave.set_address(1).await?;
    slave.init_mailbox().await?;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await?;
    group.configure(&slave).await?;
    slave.switch(CommunicationState::SafeOperational).await?;
    slave.switch(CommunicationState::Operational).await?;
    
    for _ in 0 .. 20 {
        let mut group = group.data().await;
        group.exchange().await;
        println!("received {:?}  {}  {}  {}  {}",
            group.get(restatus),
            group.get(status),
            group.get(error),
            group.get(position),
            group.get(torque),
            );
    }

    Ok(())
}
