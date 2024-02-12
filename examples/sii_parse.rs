use std::error::Error;
use etherage::{
    EthernetSocket, RawMaster,
    SlaveAddress, Slave, CommunicationState,
    sii::*,
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
        let category = categories.unpack::<CategoryHeader>().await?;
        let mut sub = categories.shadow();
        match category.ty() {
            CategoryType::General => println!("{:#?}", sub.unpack::<CategoryGeneral>().await?),
            CategoryType::SyncManager => println!("{:#?}", sub.unpack::<CategorySyncManager>().await?),
            CategoryType::PdoWrite => println!("{:#?}", sub.unpack::<CategoryPdo>().await?),
            CategoryType::PdoRead => println!("{:#?}", sub.unpack::<CategoryPdo>().await?),
            CategoryType::End => break,
            _ => println!("{:?}", category.ty()),
        }
        categories.advance(WORD*category.size());
    }

    Ok(())
}
