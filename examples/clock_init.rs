use std::{sync::Arc};
use etherage::{
    EthernetSocket,
    RawMaster,
    synchro::{SyncClock, SlaveClockConfigHelper},
    SlaveAddress,
    CommunicationState, Master, mapping, Mapping, sdo::{SyncDirection, self}, registers};
pub const SOCKET_NAME : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new(&SOCKET_NAME)?));
    {
        let master = master.clone();
        let handle = std::thread::spawn(move || loop { unsafe {master.get_raw()}.receive(); });

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
        let master = master.clone();
        let handle = std::thread::spawn(move || loop { unsafe { master.get_raw().send();} });
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

    let config = mapping::Config::default();
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

    // Search slave
    let mut iter = master.discover().await;
    let mut slaves : Vec<u16> = Vec::new();
    let mut slaves_items : Vec<etherage::Slave<'_>> = Vec::new();

    while let Some(mut s ) = iter.next().await  {
        let SlaveAddress::AutoIncremented(i) = s.address()
        else { panic!("slave already has a fixed address") };
        s.switch(CommunicationState::Init).await;
        s.set_address(i+1).await;
        s.init_mailbox().await;
        s.init_coe().await;
        if i < 3 {
            slaves.push(i + 1);
            slaves_items.push(s);
        }
    }

    // 1. Init clock
    let raw : &RawMaster = unsafe { master.get_raw() };
    let mut sc: SyncClock<'_> = SyncClock::new(raw);
    // 2. Registers slaves with DC configuration
    sc.slaves_register(&slaves, SlaveClockConfigHelper::default()).expect("Error on register");

    // 3. Initiliate clock: - Compute offset to reference clock and transmittion delay, for each registered slave
    sc.init(*slaves.first().unwrap()).await.expect("Error on init");

    master.switch(registers::AlState::PreOperational).await;
    master.group(&mapping);
    master.switch(registers::AlState::Operational).await;

    // 4. Start DC
    sc.sync().await.expect("Error on start sync");
    Ok(())
}
