/*!
	low level ethercat communication functions.
	
	It wraps an ethercat socket to schedule, send and receive ethercat frames containing data or commands.
*/

use std::{
    time::Instant,
    collections::HashMap,
    sync::{Mutex, Condvar},
    };
use core::{
    ops::DerefMut,
    time::Duration,
    };
use tokio::sync::Notify;
use bilge::prelude::*;

use crate::{
    socket::*,
    data::{self, Field, PduData, Storage, Cursor},
    };


/// maximum frame size, currently limited to the size tolerated by its header (content size coded with 11 bits)
const MAX_ETHERCAT_FRAME: usize = 2050;

/**
    low level ethercat communication functions, with no notion of slave.
    
    genericity allows to use a UDP socket or raw ethernet socket, see [crate::socket] for more details.    
    
    This struct does not do any compile-time checking of the communication states on the slaves, and has no notion of slave, it is just executing the basic commands.
    
    The ethercat low level is all about PDUs: an ethercat frame intended for slaves is a PDU frame and PDU frames contain any number of PDU (Process Data Unit), each PDU is a command, acting on one of the 2 memories types:
   
   - **Physical Memory** (aka. registers)
    
		each slave has its own physical memory, commands for physical memory (`*P*`, `B*`) are addressing a specific slave, or combining the memory reads from all slaves
		
		The physical memory is divided into registers declared in [crate::registers]
	
	- **Logical Memory** (aka. fieldbus memory)
	
		this memory doesn't physically exist anywhere, but can be read/write using `L*`  commands with each slave contributing to the record according to the configuration set before.
		
		The logical memory is organized by the mapping set in the FMMU (Fieldbust Memory Management Unit)
		
	See variants of [PduCommand] for more details.
	
	The following scheme shows an overview of the features and memory areas of every ethercat slave. Memory copy operations are represented as plain arrows regardless of the real sequence of commands needed to perform the operation. *RT* flag marks what can be acheived in realtime, and what can not.
	
	![ethercat sub protocols](/etherage/schemes/ethercat-protocols.svg)
*/
pub struct RawMaster {
	/// (µs) acceptable delay time before sending buffered PDUs
	pdu_merge_time: Duration,
	
	// socket implementation
	socket: Box<dyn EthercatSocket + Send + Sync>,
	// synchronization signal for multitask reception
	received: Notify,
	sendable: Condvar,
	send: Condvar,
	sent: Condvar,
	
	// communication state
    // states are locked using [std::sync::Mutex] since it is recommended by async-io
    // they should not be held for too long (and never during blocking operations) so they shouldn't disturb the async runtime too much
    
	pdu_state: Mutex<PduState>,
	ethercat_receive: Mutex<[u8; MAX_ETHERCAT_FRAME]>,
}
struct PduState {
	pub token: u8,
	pub last_start: usize,
	pub last_end: usize,
	pub last_time: Instant,
	pub send: [u8; MAX_ETHERCAT_FRAME],
	pub receive: HashMap<u8, PduStorage>,
}
/// struct for internal use in RawMaster
struct PduStorage {
    data: &'static [u8],
    ready: bool,
    answers: u16,
}
impl RawMaster {
	pub fn new<S: EthercatSocket + 'static + Send + Sync>(socket: S) -> Self {        
        Self {
            pdu_merge_time: std::time::Duration::from_micros(100), // microseconds
            
            socket: Box::new(socket),
            received: Notify::new(),
            sendable: Condvar::new(),
            send: Condvar::new(),
            sent: Condvar::new(),
            
            pdu_state: Mutex::new(PduState {
                token: 0,
                last_start: EthercatHeader::packed_size(),
                last_end: 0,
                last_time: Instant::now(),
                send: [0; MAX_ETHERCAT_FRAME],
                receive: HashMap::new(),
                }),
            ethercat_receive: Mutex::new([0; MAX_ETHERCAT_FRAME]),
        }
	}
	
	// shorthands to PDU commands
	// the slave address is actually packed and unpacked to actual commands again, but this is greatly shortening the code and the compiler should optimize that
	pub async fn brd<T: PduData>(&self, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Broadcast, address).await
	}
	pub async fn bwr<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Broadcast, address, data).await
	}
	pub async fn brw<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Broadcast, address, data).await
	}
	
	pub async fn aprd<T: PduData>(&self, slave: u16, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::AutoIncremented(slave), address).await
	}
	pub async fn apwr<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::AutoIncremented(slave), address, data).await
	}
	pub async fn aprw<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::AutoIncremented(slave), address, data).await
	}
	pub async fn armw(&self) {todo!()}
	
	pub async fn fprd<T: PduData>(&self, slave: u16, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Fixed(slave), address).await
	}
	pub async fn fpwr<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Fixed(slave), address, data).await
	}
	pub async fn fprw<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Fixed(slave), address, data).await
	}
	pub async fn frmw(&self) {todo!()}
	
	pub async fn lrd<T: PduData>(&self, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Logical, address).await
	}
	pub async fn lwr<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Logical, address, data).await
	}
	pub async fn lrw<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Logical, address, data).await
	}
	
	/// maps to a *rd command
	pub async fn read<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>) -> PduAnswer<T> {
        let (command, slave) = match slave {
            SlaveAddress::Broadcast => (PduCommand::BRD, 0),
            SlaveAddress::AutoIncremented(address) => (PduCommand::APRD, 0u16.wrapping_sub(address)),
            SlaveAddress::Fixed(address) => (PduCommand::FPRD, address),
            SlaveAddress::Logical => (PduCommand::LRD, 0),
            };
        let mut buffer = T::Packed::uninit();
        buffer.as_mut().fill(0);
        PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u16, &mut buffer.as_mut()[.. memory.len]).await,
			value: T::unpack(buffer.as_ref()).unwrap(),
			}
    }
	/// maps to a *wr command
	pub async fn write<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<()> {
        let (command, slave) = match slave {
            SlaveAddress::Broadcast => (PduCommand::BWR, 0),
            SlaveAddress::AutoIncremented(address) => (PduCommand::APWR, 0u16.wrapping_sub(address)),
            SlaveAddress::Fixed(address) => (PduCommand::FPWR, address),
            SlaveAddress::Logical => (PduCommand::LWR, 0),
            };
        let mut buffer = T::Packed::uninit();
        data.pack(buffer.as_mut()).unwrap();
		PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u16, &mut buffer.as_mut()[.. memory.len]).await,
			value: (),
			}
	}
	/// maps to a *rw command
	pub async fn exchange<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<T> {
        let (command, slave) = match slave {
            SlaveAddress::Broadcast => (PduCommand::BRW, 0),
            SlaveAddress::AutoIncremented(address) => (PduCommand::APRW, 0u16.wrapping_sub(address)),
            SlaveAddress::Fixed(address) => (PduCommand::FPRW, address),
            SlaveAddress::Logical => (PduCommand::LRW, 0),
            };
        let mut buffer = T::Packed::uninit();
        data.pack(buffer.as_mut()).unwrap();
        PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u16, &mut buffer.as_mut()[.. memory.len]).await,
			value: T::unpack(buffer.as_ref()).unwrap(),
			}
	}
	
	/// send a PDU on the ethercat bus
	/// the PDU is buffered with more PDUs if possible
	/// returns the number of slaves who processed the command
	pub async fn pdu(&self, command: PduCommand, slave_address: u16, memory_address: u16, data: &mut [u8]) -> u16 {
        let token;
        let (ready, _finisher) = {
            // buffering the pdu sending
            let mut state = self.pdu_state.lock().unwrap();
            
            // sending the buffer if necessary
            while MAX_ETHERCAT_FRAME < state.last_end + data.len() + PduHeader::packed_size() + PduFooter::packed_size() {
                println!("no more space, waiting");
                self.send.notify_one();
                state = self.sent.wait(state).unwrap();
            }
            
            // reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
            token = state.token;
            (state.token, _) = state.token.overflowing_add(1);
            state.receive.insert(token, PduStorage {
                // memory safety: this slice is used outside this function, but always before return
                data: unsafe {std::slice::from_raw_parts(
                        data.as_ptr() as *const u8,
                        data.len(),
                        )},
                ready: false,
                answers: 0,
                });
            
            // change last value's PduHeader.next
            if state.last_start <= state.last_end {
                let range = state.last_start .. state.last_end;
                let place = &mut state.send[range];
                let mut header = PduHeader::unpack(place).unwrap();
                header.set_next(true);
                header.pack(place).unwrap();
            }
            else {
                state.last_time = Instant::now();
                state.last_end = state.last_start;
            }
            
            // stacking the PDU in self.pdu_receive
            let advance = {
                let range = state.last_end ..;
                let mut cursor = Cursor::new(&mut state.send[range]);
                cursor.pack(&PduHeader::new(
                    u8::from(command),
                    token,
                    slave_address,
                    memory_address,
                    u11::new(data.len().try_into().unwrap()),
                    false,
                    false,
                    u16::new(0),
                    )).unwrap();
                cursor.write(data).unwrap();
                cursor.pack(&PduFooter::new(0)).unwrap();
                cursor.position()
            };
            state.last_start = state.last_end;
            state.last_end = state.last_start + advance;
            
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
        
        // waiting for the answer
        while ! *ready { self.received.notified().await; }
        
		let state = self.pdu_state.lock().unwrap();
		state.receive[&token].answers
	}
	
	/// extract a received frame of PDUs and buffer each for reception by an eventual `self.pdu()` future waiting for it.
	fn pdu_receive(&self, state: &mut PduState, frame: &[u8]) {
        let mut frame = Cursor::new(frame);
        loop {
            let header = frame.unpack::<PduHeader>().unwrap();
            if let Some(storage) = state.receive.get(&header.token()) {
                let content = frame.read(usize::from(u16::from(header.len()))).unwrap();
                
                // copy the PDU content in the reception buffer
                // concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                unsafe {std::slice::from_raw_parts_mut(
                    storage.data.as_ptr() as *mut u8,
                    storage.data.len(),
                    )}
                    .copy_from_slice(content);
                let footer = frame.unpack::<PduFooter>().unwrap();
                let storage = unsafe {&mut *(storage as *const PduStorage as *mut PduStorage)};
				storage.ready = true;
				storage.answers = footer.value; //footer.working_count();
            }
            if ! header.next() {break}
            if frame.remain().len() == 0 {todo!("raise frame error")}
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
        
        let header = EthercatHeader::unpack(frame).unwrap();
        let content = &frame[EthercatHeader::packed_size() ..];
        let content = &content[.. header.len().value() as usize];
        
        assert!(header.len().value() as usize <= content.len());
        // TODO check working count to detect possible refused requests
        match header.ty() {
            EthercatType::PDU => self.pdu_receive(
                                    self.pdu_state.lock().unwrap().deref_mut(), 
                                    content,
                                    ),
            // what is this ?
            EthercatType::NetworkVariable => todo!(),
            // no mailbox frame shall transit to this master, ignore it or raise an error ?
            EthercatType::Mailbox => {},
        }
	}
	/// this is the socket sending handler
	pub fn send(&self) {
        let state = self.pdu_state.lock().unwrap();
        let state = self.sendable.wait(state).unwrap();
        let mut state = self.send.wait_timeout(state, self.pdu_merge_time).unwrap().0;
        
        // check header
        EthercatHeader::new(
            u11::new((state.last_end - EthercatHeader::packed_size()) as u16),
            EthercatType::PDU,
            ).pack(&mut state.send).unwrap();
        
        // send
        // we are blocking the async machine until the send ends
        // we assume it is not a long operation to copy those data into the system buffer
        self.socket.send(&state.send[.. state.last_end]).unwrap();
        // reset state
        state.last_end = 0;
        state.last_start = EthercatHeader::packed_size();
        drop(state);
	}
}


/// dynamically specifies a destination address on the ethercat loop
pub enum SlaveAddress {
	/// every slave will receive and execute
	Broadcast,
	/// address will be determined by the topology (index of the slave in the ethernet loop)
	AutoIncremented(u16),
	/// address has been set by the master previously
	Fixed(u16),
	/// the logical memory is the destination, all slaves are concerned
	Logical,
}

pub struct PduAnswer<T> {
	pub answers: u16,
	pub value: T,
}
impl<T> PduAnswer<T> {
    pub fn one(self) -> T {
        self.exact(1)
    }
    pub fn exact(self, n: u16) -> T {
        assert_eq!(self.answers, n);
        self.value
    }
}


/// ethercat frame header (common to ethernet or UDP mediums) as described in ETG 1000.4 table 11
// we cannot use the packed_field macro here because the bit fiddling is weird in this header
// so here it is by hand
#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct EthercatHeader {
    /// length of the ethercat frame (minus 2 bytes, which is the header)
    len: u11,
    reserved: u1,
    /// frame type
    ty: EthercatType,
}
data::bilge_pdudata!(EthercatHeader, u16);

/// type of ethercat frame
#[bitsize(4)]
#[derive(TryFromBits, Debug, Copy, Clone)]
enum EthercatType {
    /// process data unit, use to exchange with physical and logical memory in realtime or not
    /// the mailbox content sent to slaves shall be written to the physical memory through these
    ///
    /// See ETG.1000.4
    PDU = 0x1,
    
    NetworkVariable = 0x4,
    
    /// mailbox gateway communication, between the master and non-slave devices, allowing non-slave devices to mailbox with the slaves
    /// this communication betwee, master and non-slave usually takes place in a TCP or UDP socket
    ///
    /// See ETG.8200
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
    /// slave address, its meaning depend on the command
    slave_address: u16,
    /// memory address of the data to access, which memory is accessed depend on the command
    memory_address: u16,
    /// data length following the header, excluding the footer. starting from `memory_address` in the addressed memory
    len: u11,
    reserved: u3,
    circulating: bool,
    /// true if there is an other PDU in the same PDU frame
    next: bool,
    interrupt: u16,
}
data::bilge_pdudata!(PduHeader, u80);

/// footer for PDU exchange
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Default)]
struct PduFooter {
    working_count: u16,
}
data::bilge_pdudata!(PduFooter, u16);

/// the possible PDU commands
#[bitsize(8)]
#[derive(FromBits, Debug, Copy, Clone, Default)]
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
    
    /// auto-incremented slave read
    APRD = 0x01,
    /// auto-incremented slave write
    APWR = 0x02,
    /// auto-incremented slave read & write
    APRW = 0x03,
    
    /// fixed slave read
    FPRD = 0x04,
    /// fixed slave write
    FPWR = 0x05,
    /// fixed slave read & write
    FPRW = 0x06,
    
    /// logical memory read
    LRD = 0x0A,
    /// logical memory write
    LWR = 0x0B,
    /// logical memory read & write
    LRW = 0x0C,
    
    /// auto-incremented slave read multiple write
    ARMW = 0x0D,
    /// fixed slave read multiple write
    FRMW = 0x0E,
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



