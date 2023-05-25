use std::{
    time::Instant,
    collections::HashMap,
    sync::{Mutex, Condvar},
    };
use core::ops::{Deref, DerefMut};
use core::time::Duration;
use tokio::sync::Notify;
use packed_struct::prelude::*;
use bilge::prelude::*;
use num_enum::TryFromPrimitive;
use crate::socket::*;
use crate::data::{PduData, ByteArray, PackingResult};

/**
    low level ethercat functions, with no compile-time checking of the communication states
    this struct has no notion of slave, it is just executing basic commands
    
    genericity allows to use a UDP socket or raw ethernet socket
*/
pub struct RawMaster<S: EthercatSocket> {
	/// (µs) acceptable delay time before sending buffered PDUs
	pdu_merge_time: Duration,
	
	// socket implementation
	socket: S,
	// synchronization signal for multitask reception
	received: Notify,
	sendable: Condvar,
	send: Condvar,
	sent: Condvar,
	
	// communication state
    // states are locked using [std::sync::Mutex] since it is recommended by async-io
    // they should not be held for too long (and never during blocking operations) so they shouldn't disturb the async runtime too much
    
	pdu_state: Mutex<PduState>,
	ethercat_send: Mutex<Vec<u8>>,
	ethercat_receive: Mutex<Vec<u8>>,
	ethercat_capacity: usize,
}
struct PduState {
	pub token: u8,
	pub last_start: usize,
	pub last_time: Instant,
	pub send: Vec<u8>,
	pub receive: HashMap<u8, PduStorage>,
}
impl<S: EthercatSocket> RawMaster<S> {
	pub fn new(socket: S) -> Self {
        let ethercat_capacity = 4092;
        
        Self {
            pdu_merge_time: std::time::Duration::from_micros(2000), // microseconds
            
            socket,
            received: Notify::new(),
            sendable: Condvar::new(),
            send: Condvar::new(),
            sent: Condvar::new(),
            
            pdu_state: Mutex::new(PduState {
                token: 0,
                last_start: 0,
                last_time: Instant::now(),
                send: Vec::new(),
                receive: HashMap::new(),
                }),
            ethercat_capacity,
            ethercat_send: Mutex::new(Vec::with_capacity(ethercat_capacity)),
            ethercat_receive: Mutex::new(vec![0; ethercat_capacity]),
        }
	}
	
	pub async fn bwr(&self) {todo!()}
	pub async fn brd(&self) {todo!()}
	pub async fn aprd<T: PduData>(&self, address: u16) -> T {
        self.pdu(PduCommand::APRD, 0, address, None).await
	}
	pub async fn apwr<T: PduData>(&self, address: u16, data: T) {
        self.pdu(PduCommand::APWR, 0, address, Some(data)).await;
	}
	pub async fn aprw(&self) {todo!()}
	pub async fn armw(&self) {todo!()}
	pub async fn fprd<T: PduData>(&self, slave: u16, address: u16) -> T {
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
	async fn pdu<T: PduData>(&self, command: PduCommand, destination: u16, address: u16, data: Option<T>) -> T {
//         let padding = [0u8; 30];
        
        let storage = match data {
            Some(data) => data.pack(),
            None => T::ByteArray::new(0),
            };
        let data = storage.as_bytes_slice();
        
        let mut token;
        let (ready, finisher) = {
            // buffering the pdu sending
            let mut state = self.pdu_state.lock().unwrap();
            
            // sending the buffer if necessary
            while self.ethercat_capacity < state.send.len() + data.len() + <PduHeader as PackedStruct>::ByteArray::len() {
                println!("no more space, waiting");
                self.send.notify_one();
                state = self.sent.wait(state).unwrap();
            }
            
            // reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
            token = state.token;
            (state.token, _) = state.token.overflowing_add(1);
            state.receive.insert(token, PduStorage {
                // memory safety: this slice is read only in this function or outside but before this function's return
                data: unsafe {std::slice::from_raw_parts( 
                        data.as_ptr() as *const u8,
                        data.len(),
                        )},
                ready: false,
                });
            
            // change last value's PduHeader.next
            if state.last_start < state.send.len() {
                let start = state.last_start;
                let header = PduHeader::unpack_from_slice(&state.send[start..]).unwrap()
                                    .set_next(true)
                                    .pack().unwrap();
                state.send[start..].copy_from_slice(&header);
            }
            else {
                state.last_time = Instant::now();
            }
            
            // stacking the PDU in self.pdu_receive
            state.last_start = state.send.len();
            state.send.extend_from_slice(PduHeader::new(
                command as u8,
                token,
                destination,
                address,
                u11::new(data.len().try_into().unwrap()),
                false,
                false,
                0,
                ).pack().unwrap().as_bytes_slice());
            state.send.extend_from_slice(data);
            state.send.extend_from_slice(PduFooter::new(0)
                .pack().unwrap().as_bytes_slice());
            println!("buffered pdu {} {:?}", state.last_start, state.send);
            
            self.sendable.notify_one();
        
            // memory safety: this item in the hashmap can be removed only by this function
            let ready = unsafe {&*(&state.receive[&token].ready as *const bool)};
            // clean up the receive table in case the async runtime cancels this task
            let finisher = Finisher::new(|| {
                let mut state = self.pdu_state.lock().unwrap();
                state.receive.remove(&token);
            });
            (ready, finisher)
        };
        
        println!("waiting for answer");
        // waiting for the answer
        while ! *ready { self.received.notified().await; }
        T::unpack(data).unwrap()
	}
	
	/// extract a received PDU and buffer it for reception by an eventual `self.pdu()` future waiting for it.
	fn pdu_receive(&self, state: &mut PduState, frame: &[u8]) {
        let mut frame = frame;
        loop {
            let header = PduHeader::unpack_from_slice(
                &frame[.. <PduHeader as PackedStruct>::ByteArray::len()]
                ).unwrap();
            if let Some(storage) = state.receive.get(&header.token()) {
                let content = &frame[<PduHeader as PackedStruct>::ByteArray::len() ..];
                let content = &content[.. content.len() - <PduFooter as PackedStruct>::ByteArray::len()];
                
                // copy the PDU content in the reception buffer
                // concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                unsafe {std::slice::from_raw_parts_mut(
                    storage.data.as_ptr() as *mut u8,
                    storage.data.len(),
                    )}
                    .copy_from_slice(content);
                unsafe {&mut *(storage as *const PduStorage as *mut PduStorage)}
                    .ready = true;
            }
            frame = &frame[<PduHeader as PackedStruct>::ByteArray::len() + header.len().value() as usize ..];
            if ! header.next() {break}
            if frame.len() == 0 {todo!("raise frame error")}
        }
        // the working count in the footer is useless for the master, mostly used by slaves
        self.received.notify_waiters();
    }
	
	/// this is the socket reception handler
	/// it receives and process one datagram, it may be called in loop with no particular timer since the sockets are assumed blocking
	pub fn receive(&self) {
        let mut receive = self.ethercat_receive.lock().unwrap();
        let size = self.socket.receive(receive.deref_mut()).unwrap();
        let frame = &receive[.. size];
        
        let header = EthercatHeader::unpack_from_slice(
            &frame[.. <EthercatHeader as PackedStruct>::ByteArray::len()]
            ).unwrap();
        let content = &frame[<EthercatHeader as PackedStruct>::ByteArray::len() ..];
        let content = &content[.. header.len().value() as usize];
        
        assert!(header.len().value() as usize <= content.len());
        match header.ty() {
            EthercatType::PDU => self.pdu_receive(
                                    self.pdu_state.lock().unwrap().deref_mut(), 
                                    content,
                                    ),
            EthercatType::NetworkVariable => todo!(),
            EthercatType::Mailbox => todo!(),
        }
	}
	/// this is the socket sending handler
	pub fn send(&self) {
        let mut state = self.pdu_state.lock().unwrap();
        let mut state = self.sendable.wait(state).unwrap();
        let mut state = self.send.wait_timeout(state, self.pdu_merge_time).unwrap().0;
        
        let mut send = self.ethercat_send.lock().unwrap();
        // send
        send.extend_from_slice(EthercatHeader::new(
            u11::new(state.send.len() as u16),
            EthercatType::PDU,
            ).pack().unwrap().as_bytes_slice());
        send.extend_from_slice(&state.send);
        // reset state
        state.send.clear();
        state.last_start = 0;
        drop(state);
        
        self.socket.send(send.deref()).unwrap();
        // reset send
        send.clear();
        drop(send);
	}
}




/// ethercat frame header (common to ethernet or UDP mediums) as described in ETG 1000.4 table 11
// we cannot use the packed_field macro here because the bit fiddling is weird in this header
// so here it is by hand
#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Clone)]
struct EthercatHeader {
    /// length of the ethercat frame (minus 2 bytes, which is the header)
    len: u11,
    reserved: u1,
    /// frame type
    ty: EthercatType,
}
impl PackedStruct for EthercatHeader {
    type ByteArray = [u8; 2];
    fn pack(&self) -> PackingResult<Self::ByteArray> {
        Ok(unsafe {std::mem::transmute::<&Self, &Self::ByteArray>(self)}.clone())
    }
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self> {
        Ok(unsafe {std::mem::transmute::<&Self::ByteArray, &Self>(src)}.clone())
    }
}

/// type of ethercat frame
#[bitsize(4)]
#[derive(TryFromBits, Debug, Clone)]
enum EthercatType {
    /// process data unit, use to exchange with physical and logical memory
    PDU = 0x1,
    NetworkVariable = 0x4,
    Mailbox = 0x5,
}

/// header of a PDU frame, this one of the possible ethercat frames
#[bitsize(80)]
#[derive(FromBits, DebugBits, Clone, Default)]
struct PduHeader {
    /// PDU command, specifying whether logical or physical memory is accesses, addressing type, and what read/write operation
    command: u8,
    /// PDU task request identifier
    token: u8,
    /// slave address
    destination: u16,
    /// memory address
    address: u16,
    /// data length
    len: u11,
    reserved: u3,
    circulating: bool,
    /// true if there is an other PDU in the same PDU frame
    next: bool,
    interrupt: u16,
}
impl PackedStruct for PduHeader {
    type ByteArray = [u8; 10];
    fn pack(&self) -> PackingResult<Self::ByteArray> {
        Ok(unsafe {std::mem::transmute::<&Self, &Self::ByteArray>(self)}.clone())
    }
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self> {
        Ok(unsafe {std::mem::transmute::<&Self::ByteArray, &Self>(src)}.clone())
    }
}

/// footer for PDU exchange
#[bitsize(16)]
#[derive(FromBits, DebugBits, Clone, Default)]
struct PduFooter {
    working_count: u16,
}
impl PackedStruct for PduFooter {
    type ByteArray = [u8; 2];
    fn pack(&self) -> PackingResult<Self::ByteArray> {
        Ok(unsafe {std::mem::transmute::<&Self, &Self::ByteArray>(self)}.clone())
    }
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self> {
        Ok(unsafe {std::mem::transmute::<&Self::ByteArray, &Self>(src)}.clone())
    }
}
/// the possible PDU commands
#[bitsize(8)]
#[derive(FromBits, Debug, Clone, Default)]
pub enum PduCommand {
    /// no operation
    #[fallback]
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

/// struct for internal use in RawMaster
struct PduStorage {
    data: &'static [u8],
    ready: bool,
}




struct Finisher<F: FnOnce()> {
    callback: Option<F>,
}
impl<F: FnOnce()> Finisher<F> {
    fn new(callback: F) -> Self {Self{callback: Some(callback)}}
}
impl<F: FnOnce()>
Drop for Finisher<F>  {
    fn drop(&mut self) {
        if let Some(callback) = self.callback.take() {
            callback();
        }
    }
}


// struct EthercatFrame {
//     area: Field<()>,
//     
//     fn size(data: &[u8]) -> usize {data.len() + 14}
//     fn new(&self, area) -> Self  {Self{area}}
//     fn len(&self) -> BitField<u16>    {self.area.bit_field(0, 11)}
//     fn ty(&self) -> BitField<EthercatType>  {self.area.bit_field(14, 16)}
//     fn data(&self, data: &[u8]) -> Field<()>  {self.area.field(0, self.len().get(data))}
//     fn quelqonque() -> MyStruct {}
// }
