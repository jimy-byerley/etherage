
use crate::{
    master::Master,
    rawmaster::{RawMaster, SlaveAddress},
    data::{PduData, Field},
    mailbox::Mailbox,
    can::Can,
    registers::{self, AlError},
    error::{EthercatError, EthercatResult},
    };
use magetex::*;
use core::ops::Range;
use std::sync::Arc;
use registers::AlState::*;
pub type CommunicationState = registers::AlState;


/// slave physical memory range used for mailbox, to be written by the master
const MAILBOX_BUFFER_WRITE: Range<u16> = Range {start: 0x1800, end: 0x1800+0x100};
/// slave physical memory range used for mailbox, to be read by the master
const MAILBOX_BUFFER_READ: Range<u16> = Range {start: 0x1c00, end: 0x1c00+0x100};



/**
    This struct exposes the ethercat master functions addressing one slave.

    Its lifetime refers to the [Master] the slave answers to.

    ## Note

    At contrary to [RawMaster], this struct is protocol-safe, which mean the communication cannot break because methods as not been called in the right order or at the right moment. There is nothing the user can do that might accidentally break the communication.
    The communication might however fail for hardware reasons, and the communication-safe functions shall report such errors.

    ## Example

    The following is a typical configuration sequence of a slave

    ```ignore
    slave.switch(CommunicationState::Init).await;
    slave.set_address(1).await;
    slave.init_mailbox().await;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await;
    group.configure(&slave).await;
    slave.switch(CommunicationState::SafeOperational).await;
    slave.switch(CommunicationState::Operational).await;
    ```

    In this example, `group` is a tool to manage the logical memory and mappings from [crate::mapping].
*/

pub struct Slave<'a> {
    master: Arc<RawMaster>,
    /// current address in use, fixed or topological
    address: SlaveAddress,
    /// assumed current state
    state: CommunicationState,
    /// safe master to report to if existing
    safemaster: Option<&'a Master>,

    // internal structures are inter-referencing, thus must be stored in Rc to ensure the references to it will not void because of deallocation or data move
//     sii: Mutex<Sii>,
    mailbox: Option<Arc<Mutex<Mailbox>>>,
    coe: Option<Arc<Mutex<Can>>>,
//     clock: Option<Dc>,
}
impl<'a> Slave<'a> {
    /**
        build a slave from a `RawMaster`. As everything constructed with a `RawMaster` this is not protocol-safe: no check is done, in particular nothing prevents to create multiple instances for the same physical slave, or for non-existing slaves.
        However if the physical slave is used only through one `Slave` instance, all operations will be protocol-safe.
    */
    pub fn raw(master: Arc<RawMaster>, address: SlaveAddress) -> Self {
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
    pub async fn new(master: &'a Master, address: SlaveAddress) -> EthercatResult<Slave<'a>> {
        let fixed = SlaveAddress::Fixed(master.raw.read(address, registers::address::fixed).await.one()?);
        let mut book = master.slaves.lock().await;

        if book.contains(&address) || book.contains(&fixed) {
            Err(EthercatError::Master("slave already in use by an other instance"))
        }
        else {
            book.insert(address);
            Ok(Self {
                master: master.raw.clone(),
                safemaster: Some(master),
                address,
                state: Init,
                mailbox: None,
                coe: None,
            })
        }
    }

    /// return a reference to the underlying `RawMaster` used, this method is unsafe since it allows accessing any slave concurrently to what all `Slave` and `Master` instances are doing.
    pub unsafe fn raw_master(&self) -> &Arc<RawMaster> {&self.master}

    /// retreive the slave's identification informations
    pub fn informations(&self)  {todo!()}

    /// return the current state of the slave, it does not the current expected state for this slave
    pub async fn state(&self) -> EthercatResult<CommunicationState> {
        self.master.read(self.address, registers::al::status).await.one()?
            .state().try_into()
            .map_err(|_| EthercatError::Protocol("undefined slave state"))
    }
    /// send a state change request to the slave, and return once the slave has switched
    pub async fn switch(&mut self, target: CommunicationState) -> EthercatResult<(), AlError>  {
        // send state switch request
        self.master.write(self.address, registers::al::control, {
                let mut config = registers::AlControlRequest::default();
                config.set_state(target.into());
                config.set_ack(true);
                config.set_request_id(true);
                config
		}).await.one()?;

        // wait for state change, or error
        loop {
            let status = self.master.read(self.address, registers::al::response).await.one()?;
            if status.error() {
                let error = self.master.read(self.address, registers::al::error).await.one()?;
                if error != registers::AlError::NoError
                    {return Err(EthercatError::Slave(error))}
            }
            if status.state() == target.into()  {break}
        }
        self.state = target;
		Ok(())
    }
    /**
        set the expected state of the slave.

        this actually does not perform any operation on the slave, but will change the expected behavior and thus error handling of the slave's methods
    */
    pub fn expect(&mut self, state: CommunicationState) {
        self.state = state;
    }
    /// expected state of the slave
    pub fn expected(&self) -> CommunicationState {
        self.state
    }

    /// get the current address used to communicate with the slave
    pub fn address(&self) -> SlaveAddress  {self.address}
    /// set a fixed address for the slave, `0` is forbidden
    pub async fn set_address(&mut self, fixed: u16) -> EthercatResult {
        assert!(fixed != 0);
        self.master.write(self.address, registers::address::fixed, fixed).await.one()?;
        let new = SlaveAddress::Fixed(fixed);
        // report existing address if a safemaster is used
        if let Some(safe) = self.safemaster {
            let mut book = safe.slaves.lock().await;
            book.remove(&self.address);
            book.insert(new);
        }
        self.address = new;
        Ok(())
    }
    pub async fn reset_address(self) -> EthercatResult {
        self.master.write(self.address, registers::address::fixed, 0).await.one()?;
        Ok(())
    }

    /// initialize the slave's mailbox (if supported by the slave)
    pub async fn init_mailbox(&mut self) -> EthercatResult {
        assert_eq!(self.state, Init);
        let address = match self.address {
            SlaveAddress::Fixed(i) => i,
            _ => panic!("mailbox is unsafe without fixed addresses"),
        };

        // setup the mailbox
        let mailbox = Mailbox::new(
            self.master.clone(),
            address,
            MAILBOX_BUFFER_WRITE,
            MAILBOX_BUFFER_READ,
            ).await?;

        // switch SII owner to PDI, so mailbox can init
        self.master.write(self.address, registers::sii::access, {
            let mut config = registers::SiiAccess::default();
            config.set_owner(registers::SiiOwner::Pdi);
            config
            }).await.one()?;

        self.coe = None;
        self.mailbox = Some(Arc::new(Mutex::new(mailbox)));
        Ok(())
    }

    /// initialize CoE (Canopen over Ethercat) communication (if supported by the slave), this requires the mailbox to be initialized
    pub async fn init_coe(&mut self) {
        // override the mailbox reference lifetime, we will have to make sure we free any stored object using it before destroying the mailbox
        let mailbox: Arc<Mutex<Mailbox>> = self.mailbox.clone().expect("mailbox not initialized");
        self.coe = Some(Arc::new(Mutex::new(Can::new(mailbox))));
    }

    /// locks access to CoE communication and return the underlying instance of [Can] running CoE
    pub async fn coe(&self) -> LockGuard<'_, Can> {
        self.coe
            .as_ref().expect("coe not initialized")
            .lock().await
    }

//     /// locks access to EoE communication
//     pub fn eoe(&'a self) {todo!()}

    /// read a value from the slave's physical memory
    pub async fn physical_read<T: PduData>(&self, field: Field<T>) -> EthercatResult<T>  {
        self.master.read(self.address, field).await.one()
    }
}

impl Drop for Slave<'_> {
    fn drop(&mut self) {
        // deregister from the safemaster if any
        if let Some(safe) = self.safemaster {
            let mut slaves = safe.slaves.sync_lock();
            slaves.remove(&self.address);
        }
    }
}


impl From<EthercatError<()>> for EthercatError<AlError> {
    fn from(src: EthercatError<()>) -> Self {src.upgrade()}
}
