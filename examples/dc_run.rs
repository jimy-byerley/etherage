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
use futures_concurrency::future::Join;


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // RT this thread and all child threads
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max).unwrap();
    #[cfg(target_os = "linux")]
    ioprio::set_priority(
        ioprio::Target::Process(ioprio::Pid::this()),
        Priority::new(ioprio::Class::Realtime(ioprio::RtPriorityLevel::highest())),
        ).unwrap();

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
            slave.init_coe().await.unwrap();
            
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
		offsets.push(slave.register(sdo::SyncDirection::Read, registers::dc::system_time));
		slave.channel(sdo::sync_manager.logical_read());
		slave.channel(sdo::sync_manager.logical_write());
	}
	let group = master.group(&mapping);
	
    master.switch(CommunicationState::PreOperational).await.unwrap();
	for slave in &mut slaves {
		slave.expect(CommunicationState::PreOperational);
		group.configure(&slave).await.unwrap();
	}
	
    println!("switching safeop");
    master.switch(CommunicationState::SafeOperational).await.unwrap();
    println!("safeop");
    master.switch(CommunicationState::Operational).await.unwrap();
    println!("op");

	// survey divergence
    let clock = master.clock().await;
    let mut interval = tokio_timerfd::Interval::new_interval(Duration::from_millis(2)).unwrap();
	loop {
		interval.next().await.unwrap().unwrap();
		// dynamic drift
		clock.sync().await;
		
		// survey timestamps
		let mut group = group.data().await;
		group.read().await;
		for &time in &offsets {
			print!("{} ", group.get(time));
		}
		print!("\n");
	}
}

