use std::time::Instant;
use std::collections::HashMap;
use tokio::sync::Notify;
use packed_struct::prelude::*;
use crate::socket::*;
use crate::data::{PduData, ByteArray};

/**
    low level ethercat functions, with no compile-time checking of the communication states
    this struct has no notion of slave, it is just executing basic commands
    
    genericity allows to use a UDP socket or raw ethernet socket
*/
pub struct RawMaster<S: EthercatSocket> {
	/// (µs) acceptable delay time before sending buffered PDUs
	pdu_merge_time: u128,
	
	// socket implementation
	socket: S,
	// synchronization signal for multitask reception
	received: Notify,
	
	// communication state
	pdu_token: u8,
	pdu_send_last: usize,
	pdu_time: Instant,
	pdu_send: Vec<u8>,
	pdu_receive: HashMap<u8, PduStorage>,
	
	ethercat_send: Vec<u8>,
	ethercat_receive: Vec<u8>,
}
impl<S: EthercatSocket> RawMaster<S> {
	pub fn new(socket: S) -> Self {
        let max_frame = 4092;
        
        Self {
            pdu_merge_time: 2000, // microseconds
            
            socket,
            received: Notify::new(),
            
            pdu_token: 0,
            pdu_send_last: 0,
            pdu_time: Instant::now(),
            pdu_send: Vec::new(),
            pdu_receive: HashMap::new(),
            ethercat_send: Vec::with_capacity(max_frame),
            ethercat_receive: vec![0; max_frame],
        }
	}
	
	pub async fn bwr(&self) {todo!()}
	pub async fn brd(&self) {todo!()}
	pub async fn aprd<T: PduData>(&mut self, address: u16) -> T {
        self.pdu(PduCommand::APRD, 0, address, None).await
	}
	pub async fn apwr<T: PduData>(&mut self, address: u16, data: T) {
        self.pdu(PduCommand::APWR, 0, address, Some(data)).await;
	}
	pub async fn aprw(&self) {todo!()}
	pub async fn armw(&self) {todo!()}
	pub async fn fprd<T: PduData>(&mut self, slave: u16, address: u16) -> T {
        self.pdu(PduCommand::FPRD, slave, address, None).await
	}
	pub async fn fpwr(&self) {todo!()}
	pub async fn fprw(&self) {todo!()}
	pub async fn frmw(&self) {todo!()}
	pub async fn lrd(&self) {todo!()}
	pub async fn lwr(&self) {todo!()}
	pub async fn lrw(&self) {todo!()}
	
	/// maps to a *wr command
	pub async fn write<T: PduData>(&self, address: Address, data: T) {todo!()}
	/// maps to a *rd command
	pub async fn read<T: PduData>(&self, address: Address) -> T {todo!()}
	/// maps to a *rw command
	pub async fn exchange<T: PduData>(&self, address: Address, data: T) -> T {todo!()}
	
	/// send a PDU on the ethercat bus
	/// the PDU is buffered if possible
	async fn pdu<T: PduData>(&mut self, command: PduCommand, destination: u16, address: u16, data: Option<T>) -> T {
        let padding = [0u8; 30];
        
        let storage = match data {
            Some(data) => data.pack(),
            None => T::ByteArray::new(0),
            };
        let data = storage.as_bytes_slice();
        
        // sending the buffer if necessary
        self.pdu_autoflush(data.len() + <PduHeader as PackedStruct>::ByteArray::len());
        
        // reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
        let token = self.pdu_token;
        (self.pdu_token, _) = self.pdu_token.overflowing_add(1);
        self.pdu_receive.insert(token, PduStorage {
            // memory safety: this slice is read only in this function or outside but before this function's return
            data: unsafe {std::slice::from_raw_parts( 
                    data.as_ptr() as *const u8,
                    data.len(),
                    )},
            ready: false,
            });
        
        // change last value's PduHeader.next
        if self.pdu_send_last > self.pdu_send.len() {
            let header = PduHeader::default();
            let offset = ((&header) as *const _ as usize) - ((&header.next) as *const _ as usize);
            self.pdu_send[self.pdu_send_last+offset] &= !0b1_u8;
        }
        else {
            self.pdu_time = Instant::now();
        }
        
        // stacking the PDU in self.pdu_receive
        self.pdu_send_last = self.pdu_send.len();
        self.pdu_send.extend_from_slice(PduHeader{
            command,
            token: self.pdu_token,
            destination,
            address,
            len: data.len() as u16,
            circulating: false,
            next: true,
            irq: 0,
            }.pack().unwrap().as_bytes_slice());
        self.pdu_send.extend_from_slice(data);
        if data.len() < padding.len() {
            self.pdu_send.extend_from_slice(&padding[data.len()..]);
        }
        self.pdu_send.extend_from_slice(PduFooter {
            working_count: 0,
            }.pack().unwrap().as_bytes_slice());
        
        // waiting for the answer
        let ready = &self.pdu_receive[&token].ready;
        while ! *ready { self.received.notified().await; }
        self.pdu_receive.remove(&token);
        T::unpack(data).unwrap()
        
        // TODO: clean up self in case the async runtime never finish calling this function
	}
	
	/// flush the sending buffer of PDUs if necessary
	fn pdu_autoflush(&mut self, coming: usize) {
        if self.pdu_time.elapsed().as_micros() >= self.pdu_merge_time
        || self.pdu_send.len() + coming >= self.ethercat_send.capacity() {
            self.pdu_flush();
        }
	}
	
	/// flush the sending buffer of PDUs
	fn pdu_flush(&mut self) {
        // send
        self.ethercat_send.extend_from_slice(EthercatHeader {
            len: self.pdu_send.len() as u16,
            ty: EthercatType::PDU,
            }.pack().unwrap().as_bytes_slice());
        self.ethercat_send.extend_from_slice(&self.pdu_send);
        self.socket.send(&self.ethercat_send).unwrap();
        // reset
        self.ethercat_send.clear();
        self.pdu_send.clear();
        self.pdu_send_last = 0;
	}
	
	/// extract a received PDU and buffer it for reception by an eventual `self.pdu()` future waiting for it.
	fn pdu_receive(&self, frame: &[u8]) {
        let mut frame = frame;
        loop {
            let header = PduHeader::unpack_from_slice(frame).unwrap();
            if let Some(storage) = self.pdu_receive.get(&header.token) {
                // copy the PDU content in the reception buffer
                // concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                unsafe {std::slice::from_raw_parts_mut(
                    storage.data.as_ptr() as *mut u8,
                    storage.data.len(),
                    )}
                    .copy_from_slice(frame);
                unsafe {&mut *(storage as *const PduStorage as *mut PduStorage)}
                    .ready = true;
            }
            frame = &frame[<PduHeader as PackedStruct>::ByteArray::len() + header.len as usize ..];
            if ! header.next {break}
            if frame.len() == 0 {todo!("raise frame error")}
        }
        // the working count in the footer is useless for the master, mostly used by slaves
        self.received.notify_waiters();
    }
	
	/// this is the socket reception handler
	/// it receives and process one datagram, it may be called in loop with no particular timer since the sockets are assumed blocking
	pub fn receive(&mut self) {
        let size = self.socket.receive(&mut self.ethercat_receive).unwrap();
        let frame = &self.ethercat_receive[..size];
        
        let header = EthercatHeader::unpack_from_slice(frame).unwrap();
        let content = &frame[<EthercatHeader as PackedStruct>::ByteArray::len() ..];
        assert_eq!(header.len as usize + <EthercatHeader as PackedStruct>::ByteArray::len(), frame.len());
        match header.ty {
            EthercatType::PDU => self.pdu_receive(content),
            EthercatType::NetworkVariable => todo!(),
            EthercatType::Mailbox => todo!(),
        }
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

#[derive(PackedStruct, Clone, Debug)]
#[packed_struct(size_bytes="2", bit_numbering = "lsb0", endian = "lsb")]
struct EthercatHeader {
    #[packed_field(bits="15:5")]    len: u16,
    #[packed_field(bits="4:0", ty="enum")]   ty: EthercatType,
}
/// type of ethercat frame
#[derive(PrimitiveEnum_u8, Copy, Clone, Debug, Eq, PartialEq)]
enum EthercatType {
    PDU = 0x1,
    NetworkVariable = 0x4,
    Mailbox = 0x5,
}

/// header for PDU exchange
#[derive(PackedStruct, Clone, Debug, Default)]
#[packed_struct(size_bytes="9", bit_numbering = "lsb0", endian = "lsb")]
struct PduHeader {
    #[packed_field(bytes="0", ty="enum")]  command: PduCommand,
    #[packed_field(bytes="1")]  token: u8,
    #[packed_field(bytes="2:3")]    destination: u16,
    #[packed_field(bytes="4:5")]    address: u16,
    #[packed_field(bits="48:58")]   len: u16,
    #[packed_field(bits="62")]  circulating: bool,
    #[packed_field(bits="63")]  next: bool,
    #[packed_field(bytes="8")]  irq: u8,
}
/// footer for PDU exchange
#[derive(PackedStruct, Clone, Debug)]
#[packed_struct(size_bytes="2", bit_numbering = "lsb0", endian = "lsb")]
struct PduFooter {
    #[packed_field(bytes="0:1")]  working_count: u16,
}
/// the possible PDU commands
#[derive(PrimitiveEnum_u8, Default, Copy, Clone, Debug, Eq, PartialEq)]
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

/// struct for internal use in RawMaster
struct PduStorage {
    data: &'static [u8],
    ready: bool,
}
