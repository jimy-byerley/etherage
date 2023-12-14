use std::{
    sync::Arc,
    time::Duration,
    error::Error,
//     default::Default,
    };
use etherage::{
    EthernetSocket,
    SlaveAddress,
    CommunicationState, Master, 
    registers,
    mapping,
    sdo,
    };
use ioprio::*;
use futures::stream::StreamExt;
use futures_concurrency::future::{Join, Race};


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // RT this thread and all child threads
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max).unwrap();
    #[cfg(target_os = "linux")]
    ioprio::set_priority(
        ioprio::Target::Process(ioprio::Pid::this()),
        Priority::new(ioprio::Class::Realtime(ioprio::RtPriorityLevel::highest())),
        ).unwrap();
//     #[cfg(target_os = "linux")]
//     thread_priority::set_current_thread_priority_and_policy(
//         std::os::unix::thread::JoinHandleExt::as_pthread_t(&std::thread::current()),
//         thread_priority::ThreadPriority::Max,
//         thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo),
//         ).unwrap();

    //Init master
    let master = Arc::new(Master::new(EthernetSocket::new("eno1")?));
    
    master.switch(CommunicationState::Init).await.unwrap();
    let raw = unsafe {master.get_raw()};
    (
        master.reset_addresses(),
        master.reset_clock(),
        master.reset_logical(),
        master.reset_mailboxes(),
        raw.bwr(registers::ports_errors, Default::default()),
    ).join().await;
    
    let mut tasks = Vec::new();
    let mut iter = master.discover().await;
    while let Some(mut slave) = iter.next().await  {
        tasks.push(async move {
            let SlaveAddress::AutoIncremented(i) = slave.address()
                else { panic!("slave already has a fixed address") };
			slave.expect(etherage::CommunicationState::Init);
            slave.set_address(i+1).await.unwrap();
            slave.init_mailbox().await.unwrap();
            slave.init_coe().await;
            
            slave
        });
    }
    let mut slaves = tasks.join().await;
    
    // initialize clocks and perform static drift
    master.init_clock().await.unwrap();
        
	// create a mapping buffering all divergences
	let config = mapping::Config::default();
	let mapping = mapping::Mapping::new(&config);
	let mut offsets = Vec::new();
    for slave in &slaves {
		let SlaveAddress::Fixed(i) = slave.address()
			else { panic!("slave has no fixed address") };
		let mut slave = mapping.slave(i);
		offsets.push(slave.register(sdo::SyncDirection::Read, registers::dc::system_difference));
		slave.channel(sdo::sync_manager.logical_read());
		slave.channel(sdo::sync_manager.logical_write());
	}
	let group = master.group(&mapping);
	dbg!(mapping.config());
	
    master.switch(CommunicationState::PreOperational).await.unwrap();
	for slave in &mut slaves {
		slave.expect(CommunicationState::PreOperational);
		group.configure(&slave).await.unwrap();
	}
    
//     println!("custom init");
//     for s in &slaves {
//         use etherage::Slave;
//         
//         let mut coe = s.coe().await;
//         let raw = unsafe {s.raw_master()};
//         let priority = bilge::prelude::u2::new(0);
//         
//         raw.write(s.address(), registers::isochronous::all, {
//             let mut isochronous = registers::Isochronous::default();
//             isochronous.enable.set_operation(true);
//             isochronous.enable.set_sync0(true);
// //             isochronous.interrupt0.set_enable(true);
//             isochronous.sync0_cycle_time = 2_000_000;
// //             isochronous.latch0_edge.set_positive(true);
//             isochronous
//             }).await.one().unwrap();
//         coe.sdo_write(&sdo::sync_manager.logical_write().sync().sync_mode(), priority, sdo::SyncMode::DCSync0).await.unwrap();
// //         coe.sdo_write(&sdo::sync_manager.logical_write().sync().cycle(), priority, 2_000_000).await.unwrap();
//     }
    
//     println!("switching safeop");
//     master.switch(CommunicationState::SafeOperational).await.unwrap();
//     println!("safeop");
    
    let clock = master.clock().await;
    let mut interval = tokio_timerfd::Interval::new_interval(Duration::from_millis(2)).unwrap();
	
    (
        // dynamic drift
//         clock.sync_loop(Duration::from_millis(3)),
        // survey divergence
        async { loop {
            interval.next().await.unwrap().unwrap();
            clock.sync().await;
            
            let mut group = group.data().await;
            group.read().await;
            
//             for slave in &slaves {
//                 print!("{} ", i32::from(slave.physical_read(registers::dc::system_difference).await.unwrap()));
// 			}
			for &divergence in &offsets {
                print!("{} ", i32::from(group.get(divergence)));
            }
            print!("\n");
        }},
    ).race().await;

    Ok(())
}

