
use crate::{
	master::Master,
	data::Field,
	sdo::Sdo,
	mailbox::Mailbox,
	};

#[repr(u8)]
pub enum CommunicationState {
    Init,
    PreOperational,
    SafeOperational,
    Operational,
}
use CommunicationState::*;

pub struct Slave<'a, const State: CommunicationState> {
    master: &'a Master,
    rank: u16,
    address: u16,
    mailbox_stuff: Mailbox<'a>,
}
impl<const State: CommunicationState> Slave<'a, State> {
    pub fn informations(&self)  {todo!()}
    /// return the current state of the slave
    pub fn state(&self) -> CommunicationState {todo!()}
    /// send a state change request to the slave, and return once the slave has switched
    pub fn switch<const S2: CommunicationState>(self) -> Slave<'a, S2>  {todo!()}
    /// check that the slave is in the desired communication mode, but does not switch it if not
    pub fn expect<const S2: CommunicationState>(self) -> Slave<'a, S2>  {todo!()}
    
    pub fn register_read<T>(&self, field: Field<T>) -> T  {todo!()}
}
impl Slave<'_, {Init}> {
    pub fn init(&mut self) {
        todo!()
        // setup mailbox
        // check mailbox protocols
    }
    pub fn dc(&self) {todo!()}
    pub fn coe(&self) {todo!()}
    pub fn eoe(&self) {todo!()}
}
impl Slave<'_, {PreOperational}> {
    pub fn sdo_read<T>(&self, sdo: Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T>(&self, sdo: Sdo<T>, value: T)  {todo!()}
}
impl Slave<'_, {SafeOperational}> {
    pub fn sdo_read<T>(&self, sdo: Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T>(&self, sdo: Sdo<T>, value: T)  {todo!()}
}
impl Slave<'_, {Operational}> {
    pub fn sdo_read<T>(&self, sdo: Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T>(&self, sdo: Sdo<T>, value: T)  {todo!()}
}
