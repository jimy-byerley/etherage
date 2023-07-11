use std::sync::Arc;
use core::time::Duration;
use etherage::{
    EthernetSocket,
    RawMaster,
    synchro::{SyncClock},
    SlaveAddress,
    Slave,
    CommunicationState};

pub const socket_name : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {

    let master: Arc<RawMaster> = Arc::new(RawMaster::new(EthernetSocket::new(&socket_name)?));
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
    let slaves : Vec<u16> = Vec::from([0,1,2]);

    //Init clock
    let mut sc: SyncClock<'_> = SyncClock::new(&master);
    sc.slaves_register(&slaves).expect("Error on register");
    sc.init(*slaves.first().unwrap()).await.expect("Error on init");
    sc.sync().await.expect("Error on start sync");

    println!("{}",slaves.len());
    Ok(())
}
