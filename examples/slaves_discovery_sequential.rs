use std::error::Error;
use etherage::{
    EthernetSocket, SlaveAddress, CommunicationState,
    master::Master,
    sdo,
    registers,
    };
use bilge::prelude::u2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = Master::new(EthernetSocket::new("eno1")?);
    
    master.reset_addresses().await;

    // sequencial version
    let mut iter = master.discover().await;
    while let Some(mut slave) = iter.next().await {
        let SlaveAddress::AutoIncremented(i) = slave.address()
            else {panic!("slave already has a fixed address")};
        
        let task = async {
            slave.switch(CommunicationState::Init).await?;
            slave.set_address(i+1).await?;
            slave.init_coe().await?;
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
    }

    Ok(())
}
