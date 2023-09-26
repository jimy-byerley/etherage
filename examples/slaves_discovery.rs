use std::{
    sync::Arc,
    error::Error,
    };
use futures_concurrency::future::Join;
use etherage::{
    EthernetSocket, SlaveAddress, CommunicationState,
    master::Master,
    sdo,
    registers,
    };
use bilge::prelude::u2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = Arc::new(Master::new(EthernetSocket::new("eno1")?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            unsafe {master.get_raw()}.receive().unwrap();
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            unsafe {master.get_raw()}.send().unwrap();
    })};
    
    master.reset_addresses().await;

//     // sequencial version
//     let mut iter = master.discover().await;
//     while let Some(mut slave) = iter.next().await {
//         println!("slave {:?}", slave.address());
//         let SlaveAddress::AutoIncremented(i) = slave.address()
//             else {panic!("slave already has a fixed address")};
//
//         slave.switch(CommunicationState::Init).await;
//         slave.set_address(i+1).await;
//         slave.init_mailbox().await;
//         slave.init_coe().await;
//         slave.switch(CommunicationState::PreOperational).await;
//
//         let mut can = slave.coe().await;
//         let priority = u2::new(0);
//
//         let info = slave.physical_read(registers::dl::information).await;
//         let mut name = [0; 50];
//         let mut hardware = [0; 50];
//         let mut software = [0; 50];
//
//         println!("  slave {}: {:?} - ecat type {:?} rev {:?} build {:?} - hardware {:?} software {:?}",
//                 i,
//                 std::str::from_utf8(
//                     &can.sdo_read_slice(&sdo::device_name.downcast(), priority, &mut name).await
//                     ).unwrap().trim_end(),
//                 info.ty(),
//                 info.revision(),
//                 info.build(),
//                 std::str::from_utf8(
//                     &can.sdo_read_slice(&sdo::manufacturer_hardware_version.downcast(), priority, &mut hardware).await
//                     ).unwrap().trim_end(),
//                 std::str::from_utf8(
//                     &can.sdo_read_slice(&sdo::manufacturer_software_version.downcast(), priority, &mut software).await
//                     ).unwrap().trim_end(),
//                 );
//     }

    // concurrent version
    let mut iter = master.discover().await;
    let mut pool = Vec::new();
    while let Some(mut slave) = iter.next().await {
        pool.push(async move {
            let SlaveAddress::AutoIncremented(i) = slave.address()
                else {panic!("slave already has a fixed address")};
            let task = async {
                slave.switch(CommunicationState::Init).await?;
                slave.set_address(i+1).await?;
                slave.init_mailbox().await?;
                slave.init_coe().await;
                slave.switch(CommunicationState::PreOperational).await?;
                
                let mut can = slave.coe().await;
                let priority = u2::new(0);
                
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
