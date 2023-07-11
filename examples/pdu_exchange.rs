use std::sync::Arc;
use core::time::Duration;
use futures_concurrency::future::Join;
use etherage::{Field, EthernetSocket, RawMaster};

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
    let slave = 0;

    // test read/write
    let received = master.aprd(slave, reg).await;
    master.apwr(slave, reg, received.value).await;

    // test simultaneous read/write
    (
        async {
            let received = master.aprd(slave, reg).await;
            assert_eq!(received.answers, 1);
            master.apwr(slave, reg, received.value).await;
        },
        async {
            let received = master.aprd(slave, reg).await;
            assert_eq!(received.answers, 1);
            master.apwr(slave, reg, received.value).await;
        },
    ).join().await;

    Ok(())
}
