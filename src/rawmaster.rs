use tokio::sync::Notify;
use packed_struct::prelude::PackedStruct;
use alloc::collections::{Vec, HashMap};
use crate::socket::*;
use crate::data::PackingResult;

/**
    low level ethercat functions, with no compile-time checking of the communication states
    this struct has no notion of slave, it is just executing basic commands
    
    genericity allows to use a UDP socket or raw ethernet socket
*/
pub struct RawMaster<S: EthercatSocket> {
    /// (byte) if non-null, this is the amount of PDU data to be accumulated in the send buffer before sending
	merge_pdu_size: usize,
	/// (µs) acceptable delay time before sending buffered PDUs
	merge_pdu_time: usize,
	
	// socket implementation
	socket: S,
	// synchronization signal for multitask reception
	received: Notify,
	
	// communication state
	pdu_token: u8,
	pdu_send_last: usize,
	pdu_send: Vec<u16>,
	pdu_receive: HashMap<u8, PduStorage<'static>>,
}
impl<S: EthercatSocket> RawMaster<S> {
	pub fn new() -> Self
	
	pub fn nop(&self)
	pub fn bwr(&self)
	pub fn brd(&self)
	pub fn aprd<T: PduData>(&self, address: u16) -> T {
        self.pdu(APRD, 0, address, data).await.data.unpack()
	}
	pub fn apwr(&self) {
        self.pdu(APRD, 0, address, data).await;
	}
	pub fn aprw(&self)
	pub fn armw(&self)
	pub fn fprd<T: PduData>(&self, slave: u16, address: u16) -> T {
        self.pdu(FPRD, slave, address, data).await.data.unpack()
	}
	pub fn fpwr(&self)
	pub fn fprw(&self)
	pub fn frmw(&self)
	pub fn lrd(&self)
	pub fn lwr(&self)
	pub fn lrw(&self)
	
	/// maps to a *wr command
	pub fn write(&self, address: Address, data: PduData)
	/// maps to a *rd command
	pub fn read(&self, address: Address, length: usize)
	/// maps to a *rw command
	pub fn exchange(&self, address: Address, send: Pdu, receive: usize)
	
	/// send a PDU on the ethercat bus
	/// the PDU is buffered if possible
	async fn pdu<T>(&mut self, command: u16, destination: u16, address: u16, data: T) -> u8 {
        let padding = [0u8; 30];
        
        let storage = data.pack();
        let data = data.as_bytes_slice();
        
        // reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
        let token = self.pdu_token;
        self.pdu_token = self.pdu_token.overflowing_add(1);
        self.pdu_receive.insert(token, PduStorage {
            data: data,
            ready: false,
            });
        
        // change last value's PduHeader.next
        if self.pdu_send_last > self.pdu_send.len() {
            let offset = ((&header) as *const _ as usize) - ((&header.next) as *const _ as usize);
            self.pdu_send[self.pdu_send_last+offset] &= ~0b1_u8;
        }
        
        // stacking the PDU in self.pdu_receive
        self.pdu_send_last = self.pdu_send.len();
        self.pdu_send.append(PduHeader{
            command,
            token: self.pdu_token,
            destination,
            address,
            len: data.len(),
            circulating: false,
            next: true,
            irq: 0,
            }.pack().as_bytes_slice())
        self.pdu_send.append(data);
        if data.len() < padding.len() {
            self.pdu_send.append(padding[data.len()..]);
        }
        self.pdu_send.append(PduFooter {
            wck: 0,
            }.pack().as_bytes_slice());
        
        // sending the buffer if necessary
        self.autoflush();
        // waiting for the answer
        while storage.ready { self.received.notified().await; }
        self.pdu_receive.pop(token);
        storage.data.unpack()
	}
	
	fn autoflush(&mut self) {
        if Instant::now().elapsed_since(self.tsend).as_microseconds() >= self.merge_packets_time
        || self.bsend.len() >= self.merge_packets_size {
            self.flush();
        }
	}
	
	fn flush(&mut self) {
        // send
        self.send(EthercatType::PDU, self.bsend);
        todo!()
        // reset
        self.bsend.clear();
        self.pdu_send_last = 0;
	}
}


/// dynamically specifies a destination address on the ethercat loop
pub enum Address {
	/// every slave will receive and execute
	Broadcast,
	/// address will be determined by the topology (index of the slave in the ethernet loop)
	AutoIncremented,
	/// address has been set by the master previously
	Configured(u32),
	/// the logical memory is the destination, all slaves are concerned
	Logical,
}

/// type of ethercat frame
#[derive(PackedStruct, Clone, Debug)]
#[repr(u8)]
enum EthercatType {
    PDU = 0x1,
    NetworkVariable = 0x4,
    Mailbox = 0x5,
}

/// header for PDU exchange
#[derive(PackedStruct, Clone, Debug)]
#[packed_struct(bit_numbering = "lsb0", endian = "lsb")]
struct PduHeader {
    command: PduCommand,
    token: u16,
    destination: u16,
    address: u16,
    len: u16,
    circulating: bool,
    next: bool,
    irq: u8,
}
/// footer for PDU exchange
#[derive(PackedStruct, Clone, Debug)]
#[packed_struct(bit_numbering = "lsb0", endian = "lsb")]
struct PduFooter {
    wkc: u16,
}
/// the possible PDU commands
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PduCommand {
    /// no operation
    #[default]
    NOP = 0x0,
    
    /// broadcast read
    BRD = 0x07,
    /// broadcast write
    BWR = 0x08,
    /// broadcast read & write
    BRW = 0x09,
    
    /// auto-incremented read
    APRD = 0x01,
    /// auto-incremented write
    APWR = 0x02,
    /// auto-incremented read & write
    APRW = 0x03,
    
    FPRD = 0x04,
    FPWR = 0x05,
    FPRW = 0x06,
    
    LRD = 0x0A,
    LWR = 0x0B,
    LRW = 0x0C,
    
    ARMW = 0x0D,
    FRMW = 0x0E,
}
impl packed_struct::PackedStruct for PduCommand {
    type ByteArray = [u8;1];
    fn pack(&self) -> PackingResult<Self::ByteArray>  {Ok((*self as u8).to_le_bytes())}
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self>  {Ok(u8::from_le_bytes(*src) as Self)}
}
/// struct for internal use in RawMaster
struct PduStorage<'a> {
    data: &'a [u8],
    ready: bool,
}
