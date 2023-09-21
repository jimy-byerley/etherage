use std::{sync::Arc, collections::HashMap};
use etherage::{
    EthernetSocket,
    synchro::{SyncClock, SlaveClockConfigHelper},
    SlaveAddress,
    CommunicationState, Master, Slave};
pub const SOCKET_NAME : &'static str = "eno1";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    //Init master
    let master: Arc<Master> = Arc::new(Master::new(EthernetSocket::new(&SOCKET_NAME)?));
    master.reset_addresses().await;

    // 1. Init clock and search slave
    let slaves : HashMap<u16, Slave> = fill_slave(&master).await;
    //let notify : tokio::sync::Notify = tokio::sync::Notify::new();
    //let mut sc: SyncClock = unsafe { SyncClock::new_with_ptr(master.get_raw(), &notify as *const tokio::sync::Notify) };

    // 2. Registers slaves with DC configuration
    //sc.slaves_register(&slaves, SlaveClockConfigHelper::default());
    let mut f: Foo = Foo::new();
    //'a : {  f.slaves_register(&slaves); }

    Ok(())
}

struct Info<'a> {
    n : &'a Slave<'a>

}
impl<'a> Info<'a> {
    fn new(s : &'a Slave<'a>) -> Self {
         Self { n: s}
    }
}

struct Foo<'a> {
    bar : Vec<Info<'a>>
}

impl<'a> Foo<'a> {
    fn new() -> Self { Self { bar: Vec::new() } }
    fn slaves_register(&mut self, slv : &'a HashMap<u16, Slave<'a>>) {
        for s in slv.iter()
        {
            self.bar.push(Info::new(s.1));
        }
    }
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
        if i < 3 {
            slaves.insert(i + 1,s);
        }
    }
    slaves
}
