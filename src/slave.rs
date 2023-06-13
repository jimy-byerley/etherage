
use crate::{
    master::Master,
	rawmaster::{RawMaster, SlaveAddress},
	data::{PduData, Field},
	mailbox::Mailbox,
	sdo::Sdo,
	can::Can,
	registers,
	};
use tokio::sync::{Mutex, MutexGuard};
use core::ops::Range;
use std::rc::Rc;

// #[repr(u8)]
// #[derive(Eq, PartialEq, Copy, Clone, Debug)]
// pub enum CommunicationState {
//     Init,
//     PreOperational,
//     SafeOperational,
//     Operational,
// }
// use CommunicationState::*;

pub type CommunicationState = registers::AlState;
use registers::AlState::*;

const MAILBOX_BUFFER_WRITE: Range<u16> = Range {start: 0x1800, end: 0x1800+0x100};
const MAILBOX_BUFFER_READ: Range<u16> = Range {start: 0x1c00, end: 0x1c00+0x100};



/**
	This struct exposes the ethercat master functions addressing one slave.
	
	Its lifetime refers to the [Master] the slave answers to.
	
	## Note:
	
	At contrary to [RawMaster], it is protocol-safe, which mean the communication cannot break because methods as not been called in the right order or at the right moment. There is nothing the user can do that might accidentally break the communication.
	The communication might however fail for hardware reasons, and the communication-safe functions shall report such errors.
*/
pub struct Slave<'a> {
    master: &'a RawMaster,
    address: SlaveAddress,
    state: CommunicationState,
    
    // internal structures are inter-referencing, thus must be stored in Rc to ensure the references to it will not void because of deallocation or data move
//     sii: Mutex<Sii>,
    mailbox: Option<Rc<Mutex<Mailbox<'a>>>>,
    coe: Option<Rc<Mutex<Can<'a>>>>,
//     clock: Option<Dc>,
}
impl<'a> Slave<'a> {
    pub fn new(master: &'a RawMaster, address: SlaveAddress) -> Self {
        Self {
            master,
            address,
            state: Init,
            
            mailbox: None,
            coe: None,
        }
    }
    /// retreive the slave's identification informations
    pub fn informations(&self)  {todo!()}
    
    /// return the current state of the slave
    pub async fn state(&self) -> CommunicationState {
		self.master.read(self.address, registers::al::status).await.one().state()
	}
    /// send a state change request to the slave, and return once the slave has switched
    pub async fn switch(&self, target: CommunicationState)  {
		// switch to preop
		self.master.write(self.address, registers::al::control, {
			let mut config = registers::AlControlRequest::default();
			config.set_state(target);
			config.set_ack(true);
			config
		}).await.one();
		
		loop {
			let received = self.master.read(self.address, registers::al::response).await;
			assert_eq!(received.answers, 1);
			if received.value.error() {
				let received = self.master.read(self.address, registers::al::error).await;
				assert_eq!(received.answers, 1);
				panic!("error on state change: {:?}", received.value);
			}
			if received.value.state() == target  {break}
		}
    }
    
    pub async fn set_address(&mut self, fixed: u16) {
        assert_eq!(self.state, Init);
//         {
//             let book = self.master.slaves.lock();
//             assert!(! book.contain(&fixed));
//             book.insert(fixed);
//         }
        assert!(fixed != 0);
        self.master.write(self.address, registers::address::fixed, fixed).await;
        self.address = SlaveAddress::Fixed(fixed);
    }
    
//     pub async fn auto_address(&mut self) {
//         let fixed = {
//             let book = self.master.slaves.lock();
//             let i = (0 .. book.len())
//                 .filter(|i|  book.contain(&i))
//                 .next().unwrap();
//             book.insert(i);
//             i
//             };
//         self.master.write(self.address, registers::address::fixed, fixed).await;
//     }
    
    pub async fn init_clock(&mut self)  {todo!()}

    pub async fn init_mailbox(&mut self) {
        assert_eq!(self.state, Init);
        let address = match self.address {
            SlaveAddress::Fixed(i) => i,
            _ => panic!("mailbox needs fixed addresses, setup the address first  (AFAIK)"),
        };
        // setup the mailbox
        let mailbox = Mailbox::new(
            self.master,
            address,
            MAILBOX_BUFFER_WRITE,
            MAILBOX_BUFFER_READ,
            ).await;
        // switch SII owner to PDI, so mailbox can init
		self.master.write(self.address, registers::sii::access, {
			let mut config = registers::SiiAccess::default();
			config.set_owner(registers::SiiOwner::Pdi);
			config
            }).await.one();
		
		self.coe = None;
        self.mailbox = Some(Rc::new(Mutex::new(mailbox)));
    }
    
    pub async fn init_coe(&mut self) {
        // override the mailbox reference lifetime, we will have to make sure we free any stored object using it before destroying the mailbox
        let mailbox = self.mailbox.clone().expect("mailbox not initialized");
        self.coe = Some(Rc::new(Mutex::new(Can::new(mailbox))));
    }
    
    pub async fn clock(&'a self) {todo!()}
    
    pub async fn coe(&'a self) -> MutexGuard<'a, Can<'a>>    {
        self.coe
            .as_ref().expect("coe not initialized")
            .lock().await
    }
    
    pub fn eoe(&'a self) {todo!()}
    
    pub fn physical_read<T: PduData>(&self, field: Field<T>) -> T  {todo!()}
//     pub fn physical_write<T: PduData>(&self, field: Field<T>, value: T)  {todo!()}

//     pub fn sdo_read<T: PduData>(&self, sdo: &Sdo<T>) -> T   {todo!()}
//     pub fn sdo_write<T: PduData>(&self, sdo: &Sdo<T>, value: T)  {todo!()}
}
