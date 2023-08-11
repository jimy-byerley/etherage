use std::{sync::Arc, collections::HashMap};
use etherage::{
    EthernetSocket,
    RawMaster,
    synchro::{SyncClock, SlaveClockConfigHelper},
    SlaveAddress,
    CommunicationState, Master, mapping, Mapping, sdo::{SyncDirection, self}, registers, Slave};
pub const SOCKET_NAME : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new(&SOCKET_NAME)?));
    {
        let m : Arc<Master> = master.clone();
        let _handle = std::thread::spawn(move || loop { unsafe {m.get_raw()}.receive(); });

        #[cfg(target_os = "linux")]
        {
            // let res = thread_priority::set_thread_priority_and_policy(
            //     std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
            //     thread_priority::ThreadPriority::Max,
            //     thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo)).is_ok();
            // assert_eq!(res, true);
        }
    };
    {
        let m: Arc<Master> = master.clone();
        let _handle = std::thread::spawn(move || loop { unsafe { m.get_raw().send();} });
        #[cfg(target_os = "linux")]
        {
            // let res = thread_priority::set_thread_priority_and_policy(
            //     std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
            //     thread_priority::ThreadPriority::Max,
            //     thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo)).is_ok();
            // assert_eq!(res, true);
        }
    };
    master.reset_addresses().await;

    //Mapping
    let config: etherage::Config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slavemap = mapping.slave(1);
        let _statuscom = slavemap.register(SyncDirection::Read, registers::al::status);
        let mut channel = slavemap.channel(sdo::SyncChannel{ index: 0x1c12, direction: SyncDirection::Write, capacity: 10 });
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1600, false, 10));
                let _controlword = pdo.push(sdo::cia402::controlword);
                let _target_mode = pdo.push(sdo::cia402::target::mode);
                let _target_position  = pdo.push(sdo::cia402::target::position);
                let _target_velocity = pdo.push(sdo::cia402::target::velocity);
                let _target_torque = pdo.push(sdo::cia402::target::torque);
        let mut channel = slavemap.channel(sdo::SyncChannel{ index: 0x1c13, direction: SyncDirection::Read, capacity: 10 });
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1a00, false, 10));
                let _statusword = pdo.push(sdo::cia402::statusword);
                let _error  = pdo.push(sdo::cia402::error);
                let _current_mode  = pdo.push(sdo::cia402::current::mode);
                let _current_position = pdo.push(sdo::cia402::current::position);
                let _current_velocity = pdo.push(sdo::cia402::current::velocity);
                let _current_torque  = pdo.push(sdo::cia402::current::torque);

    // 1. Init clock and search slave
    let slaves : HashMap<u16, Slave> = fill_slave(&master).await;
    let notify : tokio::sync::Notify = tokio::sync::Notify::new();
    //let raw : &RawMaster = unsafe { master.get_raw() };
    // let mut sc: SyncClock = SyncClock::new_with_ptr(raw, &notify as *const tokio::sync::Notify);
    let mut sc : SyncClock = unsafe { SyncClock::new(master.get_raw(), &notify) };

    // 2. Registers slaves with DC configuration
    //let r: usize = sc.slaves_register( &slaves as *const  HashMap<u16, Slave<'_>>, SlaveClockConfigHelper::default()).expect("Error on register");
    let r: Result<usize, etherage::EthercatError<&str>> = sc.slaves_register(&slaves, SlaveClockConfigHelper::default());
    println!("{} slave register in dc", r.unwrap());

    // 3. Initiliate clock: - Compute offset to reference clock and transmittion delay, for each registered slave
    sc.init(1).await.expect("Error on init");
    //sc.advanced_init(sm);

    master.switch(registers::AlState::PreOperational).await;
    master.group(&mapping);
    master.switch(registers::AlState::Operational).await;
    // 4. Start DC
    sc.sync().await.expect("Error on start sync");

    drop(sc);

    for s in slaves.iter() {
        println!("{}",s.0);
    }
    Ok(())
}

/// Search slave and attribute address
async fn fill_slave(master : &Master) -> HashMap<u16, Slave> {
    let mut slaves : HashMap<u16, Slave> = HashMap::new();
    let mut iter: etherage::master::SlaveDiscovery = master.discover().await;
    while let Some(mut s ) = iter.next().await  {
        let SlaveAddress::AutoIncremented(i) = s.address()
        else { panic!("slave already has a fixed address") };
        s.switch(CommunicationState::Init).await;
        s.set_address(i+1).await;
        s.init_mailbox().await;
        s.init_coe().await;
        if i < 3 {
            slaves.insert(i + 1,s);
        }
    }
    slaves
}
