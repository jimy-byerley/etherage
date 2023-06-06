use std::sync::Arc;
use core::time::Duration;
use etherage::{PduData, EthernetSocket, RawMaster};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let master = Arc::new(RawMaster::new(EthernetSocket::new("eno1")?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.receive();
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.send();
    })};
    std::thread::sleep(Duration::from_millis(500));
    
    // test read/write
//     let some: u16 = master.aprd(0x1234).await;
//     master.apwr(0x1234, some).await;
    
    // test simultaneous read/write
    let t1 = {
        let master = master.clone();
        tokio::task::spawn(async move {
            let some: u16 = master.aprd(0x1234).await;
            master.apwr(0x1234, some).await;
        })
    };
    let t2 = {
        let master = master.clone();
        tokio::task::spawn(async move {
            let some: u16 = master.aprd(0x2345).await;
            master.apwr(0x2345, some).await;
        })
    };
    t1.await.unwrap();
    t2.await.unwrap();
    
//     println!("received {:x}", some);
    Ok(())
}

