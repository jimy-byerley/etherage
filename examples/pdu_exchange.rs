use std::sync::Arc;
use core::time::Duration;
use futures_concurrency::future::Join;
use etherage::{Field, EthernetSocket, RawMaster, EthercatResult};

#[tokio::main]
async fn main() -> EthercatResult<()> {
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
    std::thread::sleep(Duration::from_millis(100));
    
    let reg = Field::<u16>::simple(0x1234);
    let slave = 0;
            
    // test read/write
    let received = master.aprd(slave, reg).await.one()?;
    master.apwr(slave, reg, received).await.one()?;
    
    // test simultaneous read/write
    (
        async {
            let received = master.aprd(slave, reg).await.one().unwrap();
            master.apwr(slave, reg, received).await.one().unwrap();
        },
        async {
            let received = master.aprd(slave, reg).await.one().unwrap();
            master.apwr(slave, reg, received).await.one().unwrap();
        },
    ).join().await;
    
    Ok(())
}

