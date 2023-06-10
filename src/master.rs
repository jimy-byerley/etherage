use crate::{
	socket::EthercatSocket,
	rawmaster::RawMaster,
	data::{PduData, Field},
	};
use bilge::prelude::*;


/**
	This struct exposes the ethercat master functions addressing the whole ethercat segment.
	Functions addressing a specific slave are exposed in [Slave]
	
	## Note:
	
	At contrary to [RawMaster], it is protocol-safe, which mean the communication cannot break because methods as not been called in the right order or at the right moment. There is nothing the user can do that might accidentally break the communication.
	The communication might however fail for hardware reasons, and the communication-safe functions shall report such errors.
*/
pub struct Master {
    raw: RawMaster,
}
impl Master {
	pub fn new<S: EthercatSocket + 'static + Send + Sync>(socket: S) -> Self {     
		Self {
			raw: RawMaster::new(socket),
		}
	}
// 	/// discover all slaves present in the ethercat segment, in topological order
//     pub async fn discover<'a>(&'a self) -> SlaveDiscovery<'a>   {todo!()}
	/// retreive a structure representing the state of all slaves in the segment
	pub fn states(&self) -> MixedState {todo!()}
	/**
		send an request for communication state change to all slaves
		the change will be effective on every slave on this function return, however [Slave::expect] will need to be called in order to convert salve instances to their proper state
	*/
	pub fn switch(&self, state: MixedState) {todo!()}
    
    /**
		return a reference to the low level master control.
		
		This method is marked unsafe since letting the user write registers may break the protocol sequences performed by the protocol implementation. Accessing the low level is communication-unsafe.
    */
    pub unsafe fn get_raw(&self) -> &RawMaster {todo!()}
    /**
		this method is safe since it consumes the communication-safe master implementation.
    */
    pub fn into_raw(self) -> RawMaster {todo!()}
    
    
    pub fn broadcast_read<T: PduData>(&self, field: Field<T>) -> T  {todo!()}
    pub fn logical_read<T: PduData>(&self, field: Field<T>) -> T  {todo!()}
    pub fn logical_write<T: PduData>(&self, field: Field<T>, value: T)   {todo!()}
    pub fn logical_exchange<T: PduData>(&self, field: Field<T>, value: T) -> T   {todo!()}
}

/**
	gather the current operation states on several devices
	this struct does not provide any way to know which slave is in which state
*/
#[bitsize(4)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct MixedState {
	pub init: bool,
	pub pre_operational: bool,
	pub safe_operational: bool,
	pub operational: bool,
}

// /// iterator of slaves in the segment, in topological order
// struct SlaveDiscovery<'a> {}
