
use crate::{
	rawmaster::RawMaster,
	data::{PduData, Field},
	sdo::Sdo,
	mailbox::Mailbox,
	registers,
	};

#[repr(u8)]
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum CommunicationState {
    Init,
    PreOperational,
    SafeOperational,
    Operational,
}
use CommunicationState::*;

// pub type CommunicationState = registers::AlState;
// use registers::AlState::*;

/**
	This struct exposes the ethercat master functions addressing one slave.
	
	Its lifetime refers to the [RawMaster] the slave answers to.
	
	## Note:
	
	At contrary to [RawMaster], it is protocol-safe, which mean the communication cannot break because methods as not been called in the right order or at the right moment. There is nothing the user can do that might accidentally break the communication.
	The communication might however fail for hardware reasons, and the communication-safe functions shall report such errors.
*/
pub struct Slave<'a, const State: CommunicationState> {
    master: &'a RawMaster,
    rank: u16,
    address: u16,
    mailbox: Option<Mailbox<'a>>,
}
impl<'a, const State: CommunicationState> Slave<'a, State> {
    pub fn informations(&self)  {todo!()}
    /// return the current state of the slave
    pub async fn state(&self) -> CommunicationState {
		self.master.fprd(self.address, registers::al::status).await.one().state()
	}
    /// send a state change request to the slave, and return once the slave has switched
    pub async fn switch<const S2: CommunicationState>(self) -> Slave<'a, S2>  {
		// switch to preop
		self.master.fpwr(self.address, registers::al::control, {
			let mut config = registers::AlControlRequest::default();
			config.set_state(S2);
			config.set_ack(true);
			config
		}).await.one();
		
		loop {
			let received = self.master.fprd(self.address, registers::al::response).await;
			assert_eq!(received.answers, 1);
	//         assert_eq!(received.value.error(), false);
			if received.value.error() {
				let received = self.master.fprd(self.address, registers::al::error).await;
				assert_eq!(received.answers, 1);
				panic!("error on state change: {:?}", received.value);
			}
			if received.value.state() == S2  {break}
		}
		
		Slave {
			master: self.master,
			rank: self.rank,
			address: self.address,
			mailbox_stuff: self.mailbox_stuff,
		}
    }
    /// check that the slave is in the desired communication mode, but does not switch it if not
    pub async fn expect<const S2: CommunicationState>(self) -> Slave<'a, S2>  {
		assert_eq!(self.state(), S2);
		Slave {
			master: self.master,
			rank: self.rank,
			address: self.address,
			mailbox_stuff: self.mailbox_stuff,
		}
    }
    
    pub fn physical_read<T: PduData>(&self, field: Field<T>) -> T  {todo!()}
}
impl Slave<'_, {Init}> {
    pub async fn init(&mut self) {
        todo!();
        // setup mailbox
        // check mailbox protocols
        // switch SII owner to PDI, so mailbox can init
		self.master.fpwr(self.address, registers::sii::access, {
			let mut config = registers::SiiAccess::default();
			config.set_owner(registers::SiiOwner::Pdi);
			config
		}).await.one();
    }
    pub fn dc(&self) {todo!()}
    pub fn coe(&self) {todo!()}
    pub fn eoe(&self) {todo!()}
}
impl Slave<'_, {PreOperational}> {
    pub fn sdo_read<T: PduData>(&self, sdo: &Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T: PduData>(&self, sdo: &Sdo<T>, value: T)  {todo!()}
}
impl Slave<'_, {SafeOperational}> {
    pub fn sdo_read<T: PduData>(&self, sdo: &Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T: PduData>(&self, sdo: &Sdo<T>, value: T)  {todo!()}
}
impl Slave<'_, {Operational}> {
    pub fn sdo_read<T: PduData>(&self, sdo: &Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T: PduData>(&self, sdo: &Sdo<T>, value: T)  {todo!()}
}
