use std::error::Error;
use etherage::{
    EthernetSocket, SlaveAddress, CommunicationState,
    master::Master,
    sdo,
    registers,
    };
use bilge::prelude::u2;
use futures_concurrency::future::Join;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    console_subscriber::init();

    let master = Master::new(EthernetSocket::new("eno1")?);
    master.reset_addresses().await;

    // concurrent version
    let mut iter = master.discover().await;
    let mut pool = Vec::new();
    while let Some(mut slave) = iter.next().await {
        pool.push(async move {
            let SlaveAddress::AutoIncremented(i) = slave.address()
                else {panic!("slave already has a fixed address")};

            let task = async {
                println!("Init task {}", i+1);
                slave.switch(CommunicationState::Init).await?;
                slave.set_address(i+1).await?;
                println!("Init mailbox {}", i+1);
                slave.init_mailbox().await?;
                println!("Init coe {}", i+1);
                slave.init_coe().await;
                println!("Switch to PreOP {}", i+1);
                slave.switch(CommunicationState::PreOperational).await?;
                println!("Lock coe {}", i+1);
                let mut can = slave.coe().await;
                let priority = u2::new(0);
                println!("Wait physical read {}", i+1);
                let info = slave.physical_read(registers::dl::information).await?;
                let mut name = [0; 50];
                let mut hardware = [0; 50];
                let mut software = [0; 50];

                println!("  slave {}: {:?} - ecat type {:?} rev {:?} build {:?} - hardware {:?} software {:?}",
                       i,
                       std::str::from_utf8(
                           &can.sdo_read_slice(&sdo::device::name, priority, &mut name).await?
                           ).unwrap().trim_end(),
                       info.ty(),
                       info.revision(),
                       info.build(),
                       std::str::from_utf8(
                           &can.sdo_read_slice(&sdo::device::hardware_version, priority, &mut hardware).await?
                           ).unwrap().trim_end(),
                       std::str::from_utf8(
                           &can.sdo_read_slice(&sdo::device::software_version, priority, &mut software).await?
                           ).unwrap().trim_end(),
                       );
                Result::<_, Box<dyn Error>>::Ok(())
            };
            if let Err(err) = task.await
                {println!("slave {}: failure: {:?}", i, err)}
        });
    }
    pool.join().await;

    Ok(())
}
