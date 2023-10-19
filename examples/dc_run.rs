use std::{
    sync::Arc,
    time::Duration,
    error::Error,
    };
use etherage::{
    EthernetSocket,
    SlaveAddress,
    CommunicationState, Master, 
    registers,
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
    
    (
//         async { unsafe {master.get_raw()}.receive().await.unwrap() },
//         async { unsafe {master.get_raw()}.send().await.unwrap() },
        async {
            master.switch(CommunicationState::Init).await.unwrap();
            (
                master.reset_addresses(),
                master.reset_clock(),
                master.reset_logical(),
        //         master.reset_mailboxes(),
                unsafe {master.get_raw()}.bwr(etherage::registers::ports_errors, etherage::registers::PortsErrorCount::default()),
            ).join().await;

            let mut tasks = Vec::new();
            let mut iter = master.discover().await;
            while let Some(mut s) = iter.next().await  {
                tasks.push(async move {
                    let SlaveAddress::AutoIncremented(i) = s.address()
                        else { panic!("slave already has a fixed address") };
                    s.set_address(i+1).await.unwrap();
                    s.init_mailbox().await.unwrap();
                    s.init_coe().await;
                    i+1
                });
            }
            let slaves = tasks.join().await;
            
            // initialize clocks and perform static drift
            master.init_clock().await.unwrap();
            
            master.switch(CommunicationState::PreOperational).await.unwrap();
        //     master.switch(CommunicationState::SafeOperational).await.unwrap();
            
            let clock = master.clock().await;
            let mut interval = tokio_timerfd::Interval::new_interval(Duration::from_millis(2)).unwrap();
            let mut interval2 = tokio_timerfd::Interval::new_interval(Duration::from_millis(5_367)).unwrap();

            (
                // dynamic drift
                clock.sync(),
                // survey divergence
                async { loop {
                    interval.next().await.unwrap().unwrap();
                    
                    for &slave in &slaves {
                        print!("{} ", clock.divergence(slave));
                    }
                    print!("\n");
                }},
                // parasitic realtime data exchanges
                async { loop {
                    interval2.next().await.unwrap().unwrap();
                    master.logical_exchange(etherage::Field::simple(0), [0u8; 64]).await;
                }},
            ).race().await.unwrap();
        },
    ).join().await;

    Ok(())
}

