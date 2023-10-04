use std::{
    sync::Arc,
    time::Duration,
    };
use etherage::{
    EthernetSocket,
    SlaveAddress,
    CommunicationState, Master, 
    registers,
    EthercatResult,
    };
use ioprio::*;
use futures_concurrency::future::{Join, Race};


#[tokio::main]
async fn main() -> EthercatResult {
    // RT this_thread
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max).unwrap();

    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new("eno1")?));
    {
        let m : Arc<Master> = master.clone();
        ioprio::set_priority(
            ioprio::Target::Process(ioprio::Pid::this()),
            Priority::new(ioprio::Class::Realtime(ioprio::RtPriorityLevel::highest())),
            ).unwrap();
        let handle = std::thread::spawn(move || loop { unsafe {m.get_raw()}.receive().unwrap(); });


        #[cfg(target_os = "linux")]
        thread_priority::set_thread_priority_and_policy(
            std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
            thread_priority::ThreadPriority::Max,
            thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo),
            ).unwrap();
    };
    {
        let m: Arc<Master> = master.clone();
        let handle = std::thread::spawn(move || loop { unsafe {m.get_raw()}.send().unwrap(); });
        #[cfg(target_os = "linux")]
        thread_priority::set_thread_priority_and_policy(
            std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
            thread_priority::ThreadPriority::Max,
            thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo),
            ).unwrap();
    };
    master.reset_addresses().await;

    let mut tasks = Vec::new();
    let mut iter = master.discover().await;
    while let Some(mut s) = iter.next().await  {
        tasks.push(async move {
            let SlaveAddress::AutoIncremented(i) = s.address()
                else { panic!("slave already has a fixed address") };
            s.switch(CommunicationState::Init).await.unwrap();
            s.set_address(i+1).await.unwrap();
            s.init_mailbox().await.unwrap();
            s.init_coe().await;
            i+1
        });
    }
    let slaves = tasks.join().await;
    
    // initialize clocks and perform static drift
    master.init_clock().await.expect("clock initialization");
    
    master.switch(registers::AlState::PreOperational).await.unwrap();
    master.switch(registers::AlState::SafeOperational).await.unwrap();
    
    let clock = master.clock().await;
    let mut interval = tokio::time::interval(Duration::from_millis(2));
    let mut interval2 = tokio::time::interval(Duration::from_micros(5_367));
    (
        // dynamic drift
        clock.sync(),
        // survey divergence
        async { loop {
            interval.tick().await;
            for &slave in &slaves {
                print!("{} ", clock.divergence(slave));
            }
            print!("\n");
        }},
        // parasitic realtime data exchanges
        async { loop {
            interval2.tick().await;
            master.logical_exchange(etherage::Field::simple(0), [0u8; 64]).await;
        }},
    ).race().await.expect("operation");

    Ok(())
}

