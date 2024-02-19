use std::error::Error;
use etherage::{
    EthernetSocket, RawMaster,
    Slave, SlaveAddress, CommunicationState,
    sdo::{self, Sdo, SyncDirection},
    mapping::{self, Mapping},
    registers,
    };

use etherage::Field;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = RawMaster::new(EthernetSocket::new("eno1")?);

    println!("create mapping");
    let config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slave = mapping.slave(1);
        let alstatus = slave.register(SyncDirection::Read, registers::al::status);
        let mut channel = slave.channel(
            sdo::SyncChannel{ direction: SyncDirection::Write, index: 0x1c12, capacity: 10 },
            // the memory buffer used for sync channels depend on slaves firmware
            0x1800 .. 0x1c00,
//             0x1000 .. 0x1400,
            );
            let mut pdo = channel.push(sdo::Pdo{ index: 0x1600, fixed: false, capacity: 10});
                let control = pdo.push(Sdo::<u16>::complete(0x6040));
                let _mode = pdo.push(Sdo::<u8>::complete(0x6060));
                let _position = pdo.push(Sdo::<i32>::complete(0x607a));
        let mut channel = slave.channel(
            sdo::SyncChannel{ direction: SyncDirection::Read, index: 0x1c13, capacity: 10 },
            // the memory buffer used for sync channels depend on slaves firmware
            0x1c00 .. 0x2000,
//             0x1400 .. 0x1800,
            );
            let mut pdo = channel.push(sdo::Pdo{ index: 0x1a00, fixed: false, capacity: 10});
                let status = pdo.push(Sdo::<u16>::complete(0x6041));
                let error = pdo.push(Sdo::<u16>::complete(0x603f));
                let position = pdo.push(Sdo::<i32>::complete(0x6064));;
    drop(slave);
    println!("done {:#?}", config);

    let allocator = mapping::Allocator::new();
    let group = allocator.group(&master, &mapping);

    println!("group {:#?}", group);
    println!("fields  {:#?}", (control, status, error, position));

    let mut slave = Slave::raw(master.clone(), SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await?;
    slave.set_address(1).await?;

    slave.init_coe().await?;

    slave.switch(CommunicationState::PreOperational).await?;
    group.configure(&slave).await?;

    slave.switch(CommunicationState::SafeOperational).await?;
    slave.switch(CommunicationState::Operational).await?;
    
    for _ in 0 .. 20 {
        let mut group = group.data().await;
        group.exchange().await;
        println!("received {}  {}  {}",
            group.get(alstatus).state(),
            group.get(status),
            group.get(position),
            );
    }

    Ok(())
}
