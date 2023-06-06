use std::sync::Arc;
use core::time::Duration;
use etherage::{PduData, Field, PduAnswer, EthernetSocket, RawMaster};

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
    
    let reg = Field::<u16>::simple(0x1234);
            
    // test read/write
    let received = master.aprd(reg).await;
    master.apwr(reg, received.value).await;
    
    // test simultaneous read/write
    let t1 = {
        let master = master.clone();
        tokio::task::spawn(async move {
            let received = master.aprd(reg).await;
            assert_eq!(received.answers, 1);
            master.apwr(reg, received.value).await;
        })
    };
    let t2 = {
        let master = master.clone();
        tokio::task::spawn(async move {
            let received = master.aprd(reg).await;
            assert_eq!(received.answers, 1);
            master.apwr(reg, received.value).await;
        })
    };
    t1.await.unwrap();
    t2.await.unwrap();
    
//     println!("received {:x}", value);
    Ok(())
}

