use std::sync::Arc;
use core::time::Duration;
use futures_concurrency::future::Join;
use etherage::{
    EthernetSocket, SlaveAddress, CommunicationState,
    master::Master,
    sdo,
    registers,
    };
use bilge::prelude::u2;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let master = Arc::new(Master::new(EthernetSocket::new("eno1")?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
//             println!("         wait data to receive");
            unsafe {master.get_raw()}.receive();
//             println!("         received");
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
//             println!("         wait data to send");
            unsafe {master.get_raw()}.send();
//             println!("         sent");
    })};
    std::thread::sleep(Duration::from_millis(500));
    
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
//         println!("  type {:?}", std::str::from_utf8(
//                 &can.sdo_read(&sdo::device_name, priority).await
//                 ).unwrap().trim_end());
//         println!("  hardware {:?}", std::str::from_utf8(
//                 &can.sdo_read(&sdo::manufacturer_hardware_version, priority).await
//                 ).unwrap().trim_end());
//         println!("  software {:?}", std::str::from_utf8(
//                 &can.sdo_read(&sdo::manufacturer_software_version, priority).await
//                 ).unwrap().trim_end());
//     }
    
    // concurrent version
    let mut iter = master.discover().await;
    let mut pool = Vec::new();
    while let Some(mut slave) = iter.next().await {
        pool.push(async move {
            let SlaveAddress::AutoIncremented(i) = slave.address() 
                else {panic!("slave already has a fixed address")};
            
            slave.switch(CommunicationState::Init).await;
            slave.set_address(i+1).await;
            slave.init_mailbox().await;
            slave.init_coe().await;
            slave.switch(CommunicationState::PreOperational).await;
            
            let mut can = slave.coe().await;
            let priority = u2::new(0);
            
            let info = slave.physical_read(registers::dl::information).await;
            
            println!("  slave {}: {:?} - ecat type {:?} rev {:?} build {:?} - hardware {:?} software {:?}", 
                    i,
                    std::str::from_utf8(
                        &can.sdo_read(&sdo::device_name, priority).await
                        ).unwrap().trim_end(),
                    info.ty(),
                    info.revision(),
                    info.build(),
                    std::str::from_utf8(
                        &can.sdo_read(&sdo::manufacturer_hardware_version, priority).await
                        ).unwrap().trim_end(),
                    std::str::from_utf8(
                        &can.sdo_read(&sdo::manufacturer_software_version, priority).await
                        ).unwrap().trim_end(),
                    );
        });
    }
    pool.join().await;
    
    Ok(())
}
