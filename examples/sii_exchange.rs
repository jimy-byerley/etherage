use std::error::Error;
use etherage::{
    EthernetSocket, RawMaster,
    SlaveAddress, Slave, CommunicationState,
    eeprom, sii::Sii,
    };

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = RawMaster::new(EthernetSocket::new("eno1")?);

    let mut slave = Slave::raw(master.clone(), SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await.unwrap();
    slave.set_address(1).await.unwrap();

    let mut sii = Sii::new(master.clone(), slave.address()).await.unwrap();

    println!("device:");
    println!("  vendor {}", sii.read(eeprom::device::vendor).await.unwrap());
    println!("  product {}", sii.read(eeprom::device::product).await.unwrap());
    println!("  revision {}", sii.read(eeprom::device::revision).await.unwrap());
    println!("  serial number: {}", sii.read(eeprom::device::serial_number).await.unwrap());
    println!("mailbox:");
    println!("  receive: \t{:#x} {:#x}",
        sii.read(eeprom::mailbox::standard::write::offset).await?,
        sii.read(eeprom::mailbox::standard::write::size).await?,
        );
    println!("  send: \t{:#x} {:#x}",
        sii.read(eeprom::mailbox::standard::read::offset).await?,
        sii.read(eeprom::mailbox::standard::read::size).await?,
        );

    println!("strings:");
    let strings = sii.strings().await.unwrap();
    for string in &strings {
        println!("  {:?}", string);
    }

    let generals = sii.generals().await.unwrap();
    println!("generals:");
    println!("  group {:?}", &strings[generals.group as usize]);
    println!("  image {:?}", &strings[generals.image as usize]);
    println!("  order {:?}", &strings[generals.order as usize]);
    println!("  name {:?}", &strings[generals.name as usize]);
    let v = generals.identification_address;
    println!("  identification address {:#x}", v);
    println!("  coe {:#?}", generals.coe);
    println!("  foe {}", generals.foe.enable());
    println!("  eoe {}", generals.foe.enable());
    println!("  flags {:#?}", generals.flags);
    let v = generals.ports;
    println!("  ports {:?}", v);

    Ok(())
}
