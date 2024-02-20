use std::error::Error;
use etherage::{
    EthernetSocket, RawMaster,
    SlaveAddress, Slave, CommunicationState,
    sii::{self, Sii, CategoryType, WORD},
    };

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = RawMaster::new(EthernetSocket::new("eno1")?);

    let mut slave = Slave::raw(master.clone(), SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await.unwrap();
    slave.set_address(1).await.unwrap();

    let mut sii = Sii::new(master.clone(), slave.address()).await.unwrap();
    sii.acquire().await?;

    let mut categories = sii.categories();
    loop {
        let category = categories.unpack::<sii::Category>().await?;
        let mut sub = categories.sub(WORD*category.size());
        match category.ty() {
            CategoryType::General => println!("{:#?}", sub.unpack::<sii::General>().await?),
            CategoryType::SyncManager => while sub.remain() > 0 {
                println!("{:#?}", sub.unpack::<sii::SyncManager>().await?);
            },
            CategoryType::SyncUnit => while sub.remain() > 0 {
                println!("{:#?}", sub.unpack::<sii::SyncUnit>().await?);
            },
            CategoryType::PdoWrite => while sub.remain() > 0 {
                let pdo = sub.unpack::<sii::Pdo>().await?;
                println!("{:#?}", pdo);
                for i in 0 .. pdo.entries {
                    let entry = sub.unpack::<sii::PdoEntry>().await?;
                    println!("    {}: {:?}", i, entry);
                }
            },
            CategoryType::PdoRead => while sub.remain() > 0 {
                let pdo = sub.unpack::<sii::Pdo>().await?;
                println!("{:#?}", pdo);
                for i in 0 .. pdo.entries {
                    let entry = sub.unpack::<sii::PdoEntry>().await?;
                    println!("    {}: {:?}", i, entry);
                }
            },
            CategoryType::Fmmu => while sub.remain() > 0 {
                println!("Fmmu {:#?}", sub.unpack::<sii::FmmuUsage>().await?);
            },
            CategoryType::FmmuExtension => while sub.remain() > 0 {
                println!("{:#?}", sub.unpack::<sii::FmmuExtension>().await?);
            },
            CategoryType::End => break,
            _ => println!("{:?}", category.ty()),
        }
    }

    Ok(())
}
