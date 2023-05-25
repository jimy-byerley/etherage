use std::{
    time::Instant,
    collections::HashMap,
    sync::{Mutex, Condvar},
    };
use core::ops::{Deref, DerefMut};
use core::time::Duration;
use tokio::sync::Notify;
use packed_struct::prelude::*;
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
            
            println!("prepare to send {:?}", command);
            
            // sending the buffer if necessary
            while self.ethercat_capacity < state.send.len() + data.len() + <PduHeader as PackedStruct>::ByteArray::len() {
                println!("no more space, waiting");
                self.send.notify_one();
                state = self.sent.wait(state).unwrap();
            }
            
            println!("buffering");
            
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
            state.send.extend_from_slice(PduFooter {
                working_count: 0,
                }.pack().unwrap().as_bytes_slice());
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
            let header = PduHeader::unpack_from_slice(frame).unwrap();
            if let Some(storage) = state.receive.get(&header.token()) {
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
        let frame = &receive[..size];
        
        let header = EthercatHeader::unpack_from_slice(frame).unwrap();
        let content = &frame[<EthercatHeader as PackedStruct>::ByteArray::len() ..];
        assert_eq!(header.len as usize + <EthercatHeader as PackedStruct>::ByteArray::len(), frame.len());
        match header.ty {
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
        println!("waiting for data to send");
        let mut state = self.sendable.wait(state).unwrap();
        println!("waiting additional data");
        let mut state = self.send.wait_timeout(state, self.pdu_merge_time).unwrap().0;
        println!("will send {:x} {:x}", state.send.len() as u16, EthercatType::PDU as u8);
        
        let mut send = self.ethercat_send.lock().unwrap();
        // send
        send.extend_from_slice(EthercatHeader {
            len: state.send.len() as u16,
            ty: EthercatType::PDU,
            }.pack().unwrap().as_bytes_slice());
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

/// ethercat frame header (common to ethernet or UDP mediums) as described in ETG 1000.4 table 11
// we cannot use the packed_field macro here because the bit fiddling is weird in this header
// so here it is by hand
#[derive(Clone, Debug)]
struct EthercatHeader {
    /// length of the ethercat frame (minus 2 bytes, which is the header)
    len: u16,   // 11 bits
    // 1 bit reserved
    /// frame type
    ty: EthercatType,  // 4 bits
}
impl PackedStruct for EthercatHeader {
    type ByteArray = [u8; 2];
    fn pack(&self) -> PackingResult<Self::ByteArray> {
        if self.len & 0xf000 != 0
            {return Err(PackingError::InvalidValue)}
        Ok((   (self.len & 0x0fff) 
            | ((self.ty as u8 as u16) << 12)
          ).to_le_bytes())
    }
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self> {
        let n = u16::from_le_bytes(src.clone());
        let len = n & 0x0fff;
        let ty = match EthercatType::try_from((n >> 12) as u8) {
            Ok(x) => x,
            Err(_) => return Err(PackingError::InvalidValue),
            };
        Ok(Self {len, ty})
    }
}

/// type of ethercat frame
#[derive(PrimitiveEnum_u8, TryFromPrimitive, Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
enum EthercatType {
    PDU = 0x1,
    NetworkVariable = 0x4,
    Mailbox = 0x5,
}

/// header for PDU exchange
// #[derive(PackedStruct, Clone, Debug, Default)]
// #[packed_struct(size_bytes="10", bit_numbering = "msb0", endian = "lsb")]
// struct PduHeader {
//     #[packed_field(bytes="0", ty="enum")]  command: PduCommand,
//     #[packed_field(bytes="1")]  token: u8,
//     #[packed_field(bytes="2:3")]    destination: u16,
//     #[packed_field(bytes="4:5")]    address: u16,
//     #[packed_field(bits="48:58")]   len: u16,
//     #[packed_field(bits="62")]  circulating: bool,
//     #[packed_field(bits="63")]  next: bool,
//     #[packed_field(bytes="8:9")]  irq: u16,
// }
use bilge::prelude::*;

#[bitsize(80)]
#[derive(FromBits, DebugBits, Clone, Default)]
struct PduHeader {
    command: u8,
    token: u8,
    destination: u16,
    address: u16,
    len: u11,
    reserved: u3,
    circulating: bool,
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

// struct EthercatFrame<'a> {
//     /// length of the ethercat frame (minus 2 bytes, which is the header)
//     len: u16,
//     ty: EthercatType,
//     data: &'a [u8],
//     
//     fn unpack(buff: &'a [u8]) -> Result<Self>
//     fn pack(&self, buff: &[u8]) -> Result
// }
// struct PduFrame<'a> {
//     command: PduCommand,
//     token: u8,
//     destination: u16,
//     address: u16,
//     len: u16,
//     circulating: bool,
//     next: bool,
//     irq: u16,
//     data: &'a [u8],
// }

// struct EthercatFrame {
//     field: Field<()>,
//     
//     fn new(&self, field) -> Self  {Self{field}}
//     fn len(&self) -> BitField<u16>    {self.field.as_bitfield().pick(BitField::new(0, 11))}
//     fn ty(&self) -> BitField<EthercatType>  {self.field.as_bitfield().pick(BitField::new(14, 16))}
//     fn data(&self, data: &[u8]) -> Field<()>  {self.field.pick(Field::new(0, self.len().get(data)))}
// }
// 
// let frame = EthercatFrame::from(data)
// match frame.len().get() {
//     match frame.ty().get() {
//         
//     }
// }
// 
// let frame = EthercatFrame::append(data)
// frame.len().set(12)
// frame.ty().set(PDU)
// let content = EthercatPDU::append(frame.data().get_mut())

// struct EthercatFrame {
//     fn size(content: usize) -> usize  {16 + content}
//     fn len() -> BitField<u16>    {BitField::new(0, 11)}
//     fn ty() -> BitField<EthercatType> {BitField::new(14, 16)}
//     fn data<'a>(data: &'a [u8]) -> &'a [u8]  {data[16 .. self.len().get(data)]}
// }
// 
// match EthercatFrame::ty().get(data) {
//     PDU => {
//         let pdu = EthercatFrame::data(frame)
//         match PduFrame::command().get(pdu)
//         },
// }

// use std::time::Duration;
// struct Event<T: Clone> {
//     mutex: std::sync::Mutex<T>,
//     cond: std::sync::Condvar,
// }
// impl Event<T: Clone> {
//     fn new(value: T) -> Self {Self{
//         mutex: std::sync::Mutex::new(value),
//         cond: std::sync::Condvar::new(),
//     }}
//     fn wait(&self) -> T {
//         let guard = self.mutex.lock().unwrap();
//         self.cond.wait(guard).unwrap().clone()
//     }
//     fn wait_timeout(&self, dur: Duration) -> T {
//         let guard = self.mutex.lock().unwrap();
//         self.cond.wait_timeout(guard, duration).unwrap().clone()
//     }
//     fn notify_one(&self, value: T) {
//         let guard = self.mutex.lock().unwrap();
//         self.cond.notify_one();
//     }
//     fn notify_all(&self, value: T) -> {
//         let guard = self.mutex.lock().unwrap();
//         self.cond.notify_all();
//     }
// }
