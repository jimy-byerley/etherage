use std::{
    time::Duration,
    error::Error,
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
    let master = Master::new(EthernetSocket::new("eno1")?);
    
    master.switch(CommunicationState::Init).await.unwrap();
    let raw = unsafe {master.get_raw()};
    (
        master.reset_addresses(),
        master.reset_clock(),
        master.reset_logical(),
        master.reset_mailboxes(),
        raw.bwr(registers::ports_errors, Default::default()),
    ).join().await;
    
    // set addresses and COE
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
    master.switch(CommunicationState::PreOperational).await.unwrap();
    
    // now that we have addresses, initialize clocks and perform static drift
    master.init_clock().await.unwrap();
        
	// create a mapping buffering all divergences
	let config = mapping::Config::default();
	let mapping = mapping::Mapping::new(&config);
	let mut offsets = Vec::new();
    for slave in &slaves {
		let SlaveAddress::Fixed(i) = slave.address()
			else { panic!("slave has no fixed address") };
		let mut slave = mapping.slave(i);
		offsets.push(slave.register(registers::SyncDirection::Read, registers::dc::system_time));
        let mut channel = slave.channel(sdo::sync_manager.logical_write(), 0x1800 .. 0x1c00);
            channel.push(sdo::Pdo::new(0x1600, false));
		let mut channel = slave.channel(sdo::sync_manager.logical_read(), 0x1c00 .. 0x2000);
            channel.push(sdo::Pdo::new(0x1600, false));
	}
	let group = master.group(&mapping).await;

	// apply mapping to slaves
	let period = Duration::from_millis(2);
	for slave in &mut slaves {
		slave.expect(CommunicationState::PreOperational);
		group.configure(&slave).await.unwrap();
		slave.init_sync(period, Duration::from_millis(1)).await.unwrap();
	}
	
    println!("switching safeop");
    master.switch(CommunicationState::SafeOperational).await.unwrap();
    println!("safeop");
    
    master.switch(CommunicationState::Operational).await.unwrap();
    println!("op");

    println!("all configured");
	// survey divergence
    let clock = master.clock().await;
	loop {
		// dynamic drift
		clock.sync().await;

        let period = period.as_nanos() as u64;
        tokio_timerfd::sleep(Duration::from_nanos(
            period - (clock.system() + clock.delay_master()) as u64 % period
            )).await.unwrap();
		
		// survey timestamps
		let mut group = group.data().await;
		group.read().await;
		for &time in &offsets {
			print!("{} ", group.get(time));
		}
		print!("\n");
	}
}

