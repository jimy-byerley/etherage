use crate::{
	socket::EthercatSocket,
	rawmaster::{RawMaster, SlaveAddress, PduAnswer},
	data::{PduData, Field},
	slave::{Slave, CommunicationState},
	mapping::{Allocator, Mapping, Group},
	clock::SyncClock,
	registers, EthercatError,
	error::EthercatResult,
	};
use std::{
    collections::HashSet,
    sync::Arc,
    };
use core::{
    ops::Range,
    default::Default,
    };
use futures_concurrency::future::Join;
use tokio::sync::RwLockReadGuard;

pub type MixedState = registers::AlMixedState;


/**
    This struct exposes the ethercat master functions addressing the whole ethercat segment.
    Functions addressing a specific slave are exposed in [Slave]

    ## Note

    At contrary to [RawMaster], this struct is protocol-safe, which mean the communication cannot break because methods as not been called in the right order or at the right moment. There is nothing the user can do that might accidentally break the communication.
    The communication might however fail for hardware reasons, and the communication-safe functions shall report such errors.

    ## Example

    The following is the typical initialization sequence of a master

    ```ignore
    Master::new(EthernetSocket::new("eno1")?)
    master.reset_addresses().await;

    let mut iter = master.discover().await;
    while let Some(mut slave) = iter.next().await {
        // check the slave
        slave.switch(CommunicationState::Init).await;
        // begin configuring the slave
        // ...
    }
    ```
*/
pub struct Master {
    pub(crate) raw: Arc<RawMaster>,
    pub(crate) slaves: std::sync::Mutex<HashSet<SlaveAddress>>,
    allocator: Allocator,
    clock: tokio::sync::RwLock<Option<SyncClock>>,
}
impl Master {
    /**
        initialize an ethercat master on the given socket
    */
    pub fn new<S: EthercatSocket + 'static + Send + Sync>(socket: S) -> Self {
        Self {
            raw: Arc::new(RawMaster::new(socket)),
            slaves: HashSet::new().into(),
            allocator: Allocator::new(),
            clock: None.into(),
        }
    }
    /**
        build a safe master from a raw master. This method is marked unsafe because it is not protocl-safe since the RawMaster can still be accessed away of this safe master instance.
    */
    pub unsafe fn raw(raw: Arc<RawMaster>) -> Self {
        Self {
            raw,
            slaves: HashSet::new().into(),
            allocator: Allocator::new(),
            clock: None.into(),
        }
    }

    /**
        return a reference to the low level master control.

        This method is marked unsafe since letting the user write registers may break the protocol sequences performed by the protocol implementation. Accessing the low level is communication-unsafe.
    */
    pub unsafe fn get_raw(&self) -> &Arc<RawMaster> {&self.raw}
//     /**
// 		this method is safe since it consumes the communication-safe master implementation.
//     */
//     pub fn into_raw(self) -> Arc<RawMaster> {self.raw}

    /**
        discover all available slaves present in the ethercat segment, in topological order

        slaves already held by a [Slave] instance will be skiped by this iterator
    */
    pub async fn discover(&self) -> SlaveDiscovery<'_>   {
        SlaveDiscovery::new(self, self.slaves().await)
    }

    /// return a reference to the master allocator of logical memory
    pub fn allocator(&self) -> &'_ Allocator {
        &self.allocator
    }

    /// allocate a group in the logical memory
    pub fn group(&'_ self, mapping: &Mapping) -> Group<'_> {
        self.allocator.group(&self.raw, mapping)
    }

    /**
        reset all slaves fixed addresses in the ethercat segment.
        
        To call this function is generally good before connecting to slaves, to allow configuring new addresses without having any previous configured addresses interfering

        this function will panic if there is instances of [Slave] alive
    */
    pub async fn reset_addresses(&self) {
        assert_eq!(self.slaves.lock().unwrap().len(), 0);
        assert!(self.clock.read().await.is_none());
        (
            self.raw.bwr(registers::address::fixed, 0),
            self.raw.bwr(registers::address::alias, 0),
        ).join().await;
    }
    /**
        reset all slaves mappings to logical memory, and sync managers channels
        
        To call this function is generally a good idea before configuring mappings on slaves, to avoid former mappings to overlap with the new ones.
        
        This function will panic if ther is instances of [Slave] alive
    */
    pub async fn reset_logical(&self) {
        assert_eq!(self.slaves.lock().unwrap().len(), 0);
        self.raw.bwr(Field::<[u8; 256]>::simple(usize::from(registers::fmmu.address)), [0; 256]).await;
        self.raw.bwr(Field::<[u8; 128]>::simple(usize::from(registers::sync_manager::interface.address)), [0; 128]).await;
    }
    /**
        reset mailbox configurations for all slaves, freeing their reserved memory space.
        
        This function will panic if ther is instances of [Slave] alive
    */
    pub async fn reset_mailboxes(&self) {
        assert_eq!(self.slaves.lock().unwrap().len(), 0);
        self.raw.bwr(Field::<[u8; 256]>::simple(usize::from(registers::sync_manager::interface.address)), [0; 256]).await;
    }
    /**
        reset all slaves clock synchronization configurations
        
        To call this function is generally a good idea during slaves configuration if you are not using clock at all because preexisting clock setup may prevent slaves from switching to operation mode.
    */
    pub async fn reset_clock(&self) {
        // stop and void the previous clock
        self.clock.write().await.take().map(|clock| clock.stop());
        // reset registers
        (
            self.raw.bwr(registers::dc::clock, Default::default()),
            self.raw.bwr(registers::isochronous::slave_cfg, Default::default()),
        ).join().await;
    }
    
    /// initialize distributed clock on all slaves that support it
    pub async fn init_clock(&self) -> Result<(), EthercatError> {
        self.reset_clock().await;
        self.clock.write().await.replace(
            SyncClock::all(self.raw.clone(), None, None).await?
            );
        Ok(())
    }
    
    /// return the underlying instance of [SyncClock] synchronizing slaves clocks
    pub async fn clock(&self) -> RwLockReadGuard<'_, SyncClock> {
        RwLockReadGuard::map(
            self.clock.read().await, 
            |o|  o.as_ref().expect("clock not initialized"),
            )
    }
    
    /// number of slaves in the ethercat segment (only answering slaves will be accounted for)
    pub async fn slaves(&self) -> u16 {
        self.raw.brd(registers::al::status).await.answers
    }

    /// retreive a structure representing the state of all slaves in the segment
    pub async fn states(&self) -> MixedState {
        self.raw.brd(registers::al::status).await.value().unwrap().state()
    }
    /**
        send an request for communication state change to all slaves.

        the change will be effective on every slave on this function return, however [Slave::expect] will need to be called in order to convert salve instances to their proper state
    */
    pub async fn switch(&self, target: CommunicationState) -> EthercatResult {
        self.raw.bwr(registers::al::control, {
            let mut config = registers::AlControlRequest::default();
            config.set_state(target.into());
            config.set_ack(true);
            config.set_request_id(true);
            config
        }).await;
        
        // wait for state change, or error
        loop {
            let status = self.raw.brd(registers::al::response).await.value().unwrap();
            if status.error() 
				{return Err(EthercatError::Slave(()))}
//             print!("slaves state {:?}  waiting {:?}     ",
//                 status.state(),
//                 target,
//                 );
            if status.state() == target.into()  
                {break}
        }
        Ok(())
    }


    /// same as [RawMaster::brd]
    pub async fn broadcast_read<T: PduData>(&self, field: Field<T>) -> PduAnswer<T>  {self.raw.brd(field).await}
    /// same as [RawMaster::lrd]
    pub async fn logical_read<T: PduData>(&self, field: Field<T>) -> PduAnswer<T>  {self.raw.lrd(field).await}
    /// same as [RawMaster::lwr]
    pub async fn logical_write<T: PduData>(&self, field: Field<T>, value: T) -> PduAnswer<()>   {self.raw.lwr(field, value).await}
    /// same as [RawMaster::lrw]
    pub async fn logical_exchange<T: PduData>(&self, field: Field<T>, value: T) -> PduAnswer<T>   {self.raw.lrw(field, value).await}
}


/// iterator of unused slaves in the segment, in topological order
pub struct SlaveDiscovery<'a> {
    master: &'a Master,
    iter: Range<u16>,
}
impl<'a> SlaveDiscovery<'a> {
    fn new(master: &'a Master, max: u16) -> Self {
        Self {
            master,
            iter: 0 .. max,
        }
    }
    /// next method lile in an iterator, except this method is async
    pub async fn next(&mut self) -> Option<Slave<'a>> {
        while let Some(address) = self.iter.next() {
            if let Ok(slave) = Slave::new(&self.master, SlaveAddress::AutoIncremented(address)).await
                {return Some(slave)}
        }
        None
    }
}
