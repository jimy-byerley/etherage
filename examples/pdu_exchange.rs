use futures_concurrency::future::Join;
use etherage::{Field, EthernetSocket, RawMaster, EthercatResult};

#[tokio::main]
async fn main() -> EthercatResult<()> {
    let master = RawMaster::new(EthernetSocket::new("eno1")?);

    let reg = Field::<u16>::simple(0x1234);
    let slave = 0;

    // test read/write
    dbg!(1);
    let received = master.aprd(slave, reg).await.one().unwrap();
    dbg!(2);
    master.apwr(slave, reg, received).await.one().unwrap();
    dbg!(3);

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
