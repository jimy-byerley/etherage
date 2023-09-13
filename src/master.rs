use crate::{
	socket::EthercatSocket,
	rawmaster::{RawMaster, SlaveAddress, PduAnswer},
	data::{PduData, Field},
	slave::{Slave, CommunicationState},
	mapping::{Allocator, Mapping, Group},
	registers,
	};
use std::{
    collections::HashSet,
    sync::Mutex,
    };
use core::ops::Range;

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
    pub(crate) raw: RawMaster,
    pub(crate) slaves: Mutex<HashSet<SlaveAddress>>,
    allocator: Allocator,
}
impl Master {
	pub fn new<S: EthercatSocket + 'static + Send + Sync>(socket: S) -> Self {
		Self {
			raw: RawMaster::new(socket),
			slaves: Mutex::new(HashSet::new()),
			allocator: Allocator::new(),
		}
	}

    /**
		return a reference to the low level master control.

		This method is marked unsafe since letting the user write registers may break the protocol sequences performed by the protocol implementation. Accessing the low level is communication-unsafe.
    */
    pub unsafe fn get_raw(&self) -> &'_ RawMaster {&self.raw}
    /**
		this method is safe since it consumes the communication-safe master implementation.
    */
    pub fn into_raw(self) -> RawMaster {self.raw}

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

        this function will panic if there is instances of [Slave] alive
    */
    pub async fn reset_addresses(&self) {
        assert_eq!(self.slaves.lock().unwrap().len(), 0);
        self.raw.bwr(registers::address::fixed, 0).await;
    }
    pub async fn reset_logical(&self) {todo!()}
    pub async fn reset_mailboxes(&self) {todo!()}
    
    /// number of slaves in the ethercat segment (only answering slaves will be accounted for)
    pub async fn slaves(&self) -> u16 {
        self.raw.brd(registers::al::status).await.answers
    }

	/// retreive a structure representing the state of all slaves in the segment
	pub async fn states(&self) -> MixedState {
        self.raw.brd(registers::al::status).await.value.state()
	}
	/**
		send an request for communication state change to all slaves.

		the change will be effective on every slave on this function return, however [Slave::expect] will need to be called in order to convert salve instances to their proper state
	*/
	pub async fn switch(&self, target: CommunicationState) {
        self.raw.bwr(registers::al::control, {
			let mut config = registers::AlControlRequest::default();
			config.set_state(target.into());
			config.set_ack(true);
			config
		}).await;
        // TODO: wait until all slaves switched ?
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
            if let Some(slave) = Slave::new(&self.master, SlaveAddress::AutoIncremented(address)).await
                {return Some(slave)}
        }
        None
    }
}
