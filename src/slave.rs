
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
use std::sync::Arc;



pub type CommunicationState = registers::AlState;
use registers::AlState::*;


/// slave physical memory range used for mailbox, to be written by the master
const MAILBOX_BUFFER_WRITE: Range<u16> = Range {start: 0x1800, end: 0x1800+0x100};
/// slave physical memory range used for mailbox, to be read by the master
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
    /// current address in use, fixed or topological
    address: SlaveAddress,
    /// assumed current state
    state: CommunicationState,
    /// safe master to report to if existing
    safemaster: Option<&'a Master>,
    
    // internal structures are inter-referencing, thus must be stored in Rc to ensure the references to it will not void because of deallocation or data move
//     sii: Mutex<Sii>,
    mailbox: Option<Arc<Mutex<Mailbox<'a>>>>,
    coe: Option<Arc<Mutex<Can<'a>>>>,
//     clock: Option<Dc>,
}
impl<'a> Slave<'a> {
    /**
        build a slave from a `RawMaster`. As everything constructed with a `RawMaster` this is not protocol-safe: no check is done, in particular nothing prevents to create multiple instances for the same physical slave, or for non-existing slaves.
        However if the physical slave is used only through one `Slave` instance, all operations will be protocol-safe.
    */
    pub fn raw(master: &'a RawMaster, address: SlaveAddress) -> Self {
        Self {
            master,
            safemaster: None,
            address,
            state: Init,
            
            mailbox: None,
            coe: None,
        }
    }
    /**
        build a slave from a `Master`. exclusive acces to the addressed slave is ensured by `Master`, and the use of this struct will be protocl-safe.
    */
    pub async fn new(master: &'a Master, address: SlaveAddress) -> Option<Slave<'a>> {
        let mut book = master.slaves.lock().unwrap();
        if book.contains(&address)
        || book.contains(&SlaveAddress::Fixed(
                master.raw.read(address, registers::address::fixed).await.one()
                ))
            {None}
        else {
            book.insert(address);
            Some(Self {
                master: &master.raw,
                safemaster: Some(master),
                address,
                state: Init,
                
                mailbox: None,
                coe: None,
            })
        }
    }
    
    /// return a reference to the underlying `RawMaster` used, this method is unsafe since it allows accessing any slave concurrently to what all `Slave` and `Master` instances are doing.
    pub unsafe fn raw_master(&self) -> &'a RawMaster {self.master}
    
    /// retreive the slave's identification informations
    pub fn informations(&self)  {todo!()}
    
    /// return the current state of the slave, it does not the current expected state for this slave
    pub async fn state(&self) -> CommunicationState {
		self.master.read(self.address, registers::al::status).await.one()
            .state().try_into().unwrap()
	}
    /// send a state change request to the slave, and return once the slave has switched
    pub async fn switch(&mut self, target: CommunicationState)  {
		// send state switch request
		self.master.write(self.address, registers::al::control, {
			let mut config = registers::AlControlRequest::default();
			config.set_state(target.into());
			config.set_ack(true);
			config
		}).await.one();
		
		// wait for state change, or error
		loop {
			let received = self.master.read(self.address, registers::al::response).await.one();
			if received.error() {
				let received = self.master.read(self.address, registers::al::error).await.one();
				if received == registers::AlError::NoError  {break}
				panic!("error on state change: {:?}", received);
			}
			if received.state() == target.into()  {break}
		}
		self.state = target;
    }
    /**
        set the expected state of the slave.
        
        this actually does not perform any operation on the slave, but will change the expected behavior and thus error handling of the slave's methods
    */
    pub fn expect(&mut self, state: CommunicationState) {
        self.state = state;
    }
    
    /// get the current address used to communicate with the slave
    pub fn address(&self) -> SlaveAddress  {self.address}
    /// set a fixed address for the slave, `0` is forbidden
    pub async fn set_address(&mut self, fixed: u16) {
        assert_eq!(self.state, Init);
        assert!(fixed != 0);
        self.master.write(self.address, registers::address::fixed, fixed).await;
        let new = SlaveAddress::Fixed(fixed);
        // report existing address if a safemaster is used
        if let Some(safe) = self.safemaster {
            let mut book = safe.slaves.lock().unwrap();
            book.remove(&self.address);
            book.insert(new);
        }
        self.address = new;
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

    /// initialize the slave's mailbox (if supported by the slave)
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
        self.mailbox = Some(Arc::new(Mutex::new(mailbox)));
    }
    
    /// initialize CoE (Canopen over Ethercat) communication (if supported by the slave), this requires the mailbox to be initialized
    pub async fn init_coe(&mut self) {
        // override the mailbox reference lifetime, we will have to make sure we free any stored object using it before destroying the mailbox
        let mailbox = self.mailbox.clone().expect("mailbox not initialized");
        self.coe = Some(Arc::new(Mutex::new(Can::new(mailbox))));
    }
    
    pub async fn clock(&'a self) {todo!()}
    
    /// locks access to CoE communication and return the underlying instance of [Can] running CoE
    pub async fn coe(&self) -> MutexGuard<'_, Can<'a>>    {
        self.coe
            .as_ref().expect("coe not initialized")
            .lock().await
    }
    
    /// locks access to EoE communication
    pub fn eoe(&'a self) {todo!()}
    
    /// read a value from the slave's physical memory
    pub async fn physical_read<T: PduData>(&self, field: Field<T>) -> T  {
        self.master.read(self.address, field).await.one()
    }
}

impl Drop for Slave<'_> {
    fn drop(&mut self) {
        // deregister from the safemaster if any
        if let Some(safe) = self.safemaster {
            let mut book = safe.slaves.lock().unwrap();
            book.remove(&self.address);
        }
    }
}
