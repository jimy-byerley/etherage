use std::{
    sync::Arc, 
    collections::HashMap,
    };
use etherage::{
    EthernetSocket,
    synchro::{SyncClock, SlaveClockConfigHelper},
    SlaveAddress,
    CommunicationState, Master, mapping, Mapping, 
    sdo::{SyncDirection, self}, 
    registers, Slave,
    };
use ioprio::*;

pub const SOCKET_NAME : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // RT this_thread
    assert!(thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max).is_ok());

    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new(&SOCKET_NAME)?));
    {
        let m : Arc<Master> = master.clone();
        ioprio::set_priority(
            ioprio::Target::Process(ioprio::Pid::this()),
            Priority::new(ioprio::Class::Realtime(ioprio::RtPriorityLevel::highest()) ));
        let handle = std::thread::spawn(move || loop { unsafe {m.get_raw()}.receive(); });


        #[cfg(target_os = "linux")]
        {
            let res = thread_priority::set_thread_priority_and_policy(
                std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
                thread_priority::ThreadPriority::Max,
                thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo)).is_ok();
            assert_eq!(res, true);
        }
    };
    {
        let m: Arc<Master> = master.clone();
        let handle = std::thread::spawn(move || loop { unsafe { m.get_raw().send();} });
        #[cfg(target_os = "linux")]
        {
            let res = thread_priority::set_thread_priority_and_policy(
                std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
                thread_priority::ThreadPriority::Max,
                thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo)).is_ok();
            assert_eq!(res, true);
        }
    };
    master.reset_addresses().await;

    // 1. Init clock and search slave
    let slaves = fill_slave(&master).await;
    let mut sc = SyncClock::new(unsafe {master.get_raw()});

    // 2. Registers slaves with DC configuration
    let r = sc.slaves_register(&slaves, SlaveClockConfigHelper::default()) .unwrap();
    println!("{} slave register in dc", r);

    // 3. Initiliate clock: - Compute offset to reference clock and transmittion delay, for each registered slave
    sc.init(1).await.expect("Error on init");

    master.switch(registers::AlState::PreOperational).await;
    master.switch(registers::AlState::SafeOperational).await;
    // 4. Start DC
    sc.sync().await.expect("Error on start sync");

    Ok(())
}

/// Search slave and attribute address
async fn fill_slave(master : &Master) -> HashMap<u16, Slave> {
    let mut slaves : HashMap<u16, Slave> = HashMap::new();
    let mut iter: etherage::master::SlaveDiscovery = master.discover().await;
    while let Some(mut s) = iter.next().await  {
        let SlaveAddress::AutoIncremented(i) = s.address()
            else { panic!("slave already has a fixed address") };
        if i >= 7  {break}
        s.switch(CommunicationState::Init).await;
        s.set_address(i+1).await;
        s.init_mailbox().await;
        s.init_coe().await;
        slaves.insert(i+1, s);
    }
    slaves
}
