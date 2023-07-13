use std::sync::Arc;
use core::time::Duration;
use etherage::{
    EthernetSocket,
    RawMaster,
    synchro::{SyncClock},
    SlaveAddress,
    Slave,
    CommunicationState, Master};

pub const SOCKET_NAME : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new(&SOCKET_NAME)?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            unsafe {master.get_raw()}.receive();
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            unsafe { master.get_raw().send();}
    })};
    master.reset_addresses().await;

    // Search slave
    let mut iter = master.discover().await;
    let mut slaves : Vec<u16> = Vec::new();
    while let Some(mut s ) = iter.next().await  {
        let SlaveAddress::AutoIncremented(i) = s.address()
        else { panic!("slave already has a fixed address") };
        s.set_address(i+1).await;
        s.switch(CommunicationState::Init).await;
        slaves.push(i + 1);
    }

    //Init clock
    let raw : &RawMaster = unsafe { master.get_raw() };
    let mut sc: SyncClock<'_> = SyncClock::new(raw);
    sc.slaves_register(&slaves).expect("Error on register");
    sc.init(*slaves.first().unwrap()).await.expect("Error on init");
    sc.sync().await.expect("Error on start sync");

    //slave.switch(CommunicationState::PreOperational).await;

    println!("{}",slaves.len());
    Ok(())
}
