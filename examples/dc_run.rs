use std::sync::Arc;
use etherage::{
    EthernetSocket,
    SlaveAddress,
    CommunicationState, Master, 
    registers,
    };
use ioprio::*;
use futures_concurrency::future::Join;

pub const SOCKET_NAME : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // RT this_thread
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max).unwrap();

    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new(&SOCKET_NAME)?));
    {
        let m : Arc<Master> = master.clone();
        ioprio::set_priority(
            ioprio::Target::Process(ioprio::Pid::this()),
            Priority::new(ioprio::Class::Realtime(ioprio::RtPriorityLevel::highest())),
            ).unwrap();
        let handle = std::thread::spawn(move || loop { unsafe {m.get_raw()}.receive(); });


        #[cfg(target_os = "linux")]
        thread_priority::set_thread_priority_and_policy(
            std::os::unix::thread::JoinHandleExt::as_pthread_t(&handle),
            thread_priority::ThreadPriority::Max,
            thread_priority::ThreadSchedulePolicy::Realtime(thread_priority::RealtimeThreadSchedulePolicy::Fifo),
            ).unwrap();
    };
    {
        let m: Arc<Master> = master.clone();
        let handle = std::thread::spawn(move || loop { unsafe { m.get_raw().send();} });
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
            s.switch(CommunicationState::Init).await;
            s.set_address(i+1).await;
            s.init_mailbox().await;
            s.init_coe().await;
        });
    }
    tasks.join().await;
    
    master.init_clock().await.expect("clock initialization");
    
    master.switch(registers::AlState::PreOperational).await;
    master.switch(registers::AlState::SafeOperational).await;
    
    master.clock().await.sync().await.expect("clock synchronization task");

    Ok(())
}

