/*!
	low level ethercat communication functions.
	
	It wraps an ethercat socket to schedule, send and receive ethercat frames containing data or commands.
*/

use std::sync::{Mutex, Condvar};
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


/// maximum frame size, currently limited to the ethernet frame size
// size tolerated by its header (content size coded with 11 bits)
// const MAX_ETHERCAT_FRAME: usize = 2050;
// size tolerated by ethernet encapsulation
const MAX_ETHERCAT_FRAME: usize = 1500;
/// minimum PDU size
const MIN_PDU: usize = 60;
/// maximum number of PDU in an ethercat frame
const MAX_ETHERCAT_PDU: usize = MAX_ETHERCAT_FRAME / MIN_PDU;

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
		
		The logical memory is organized by the mapping set in the FMMU (Fieldbust Memory Management Unit). [crate::mapping] helps on this task.
		
	See variants of [PduCommand] and [Self::pdu] for more details.
	
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
	sent: Notify,
	
	// communication state
    // states are locked using [std::sync::Mutex] since it is recommended by async-io
    // they should not be held for too long (and never during blocking operations) so they shouldn't disturb the async runtime too much
    
	pdu_state: Mutex<PduState>,
	ethercat_receive: Mutex<[u8; MAX_ETHERCAT_FRAME]>,
}
struct PduState {
	last_start: usize,
	last_end: usize,
	ready: bool,
	/// send buffer, it contains one ethercat frame
	send: [u8; MAX_ETHERCAT_FRAME],
	/// reception destination, each containing a reception buffer and additional infos
	receive: [Option<PduStorage>; 2*MAX_ETHERCAT_PDU],
	/// list of free reception storages
	free: heapless::Vec<usize, {2*MAX_ETHERCAT_PDU}>,
}
/// struct for internal use in RawMaster
struct PduStorage {
    data: &'static mut [u8],
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
            sent: Notify::new(),
            
            pdu_state: Mutex::new(PduState {
                last_start: EthercatHeader::packed_size(),
                last_end: 0,
                ready: false,
                send: [0; MAX_ETHERCAT_FRAME],
                receive: [0; 2*MAX_ETHERCAT_PDU].map(|_| None),
                free: (0 .. 2*MAX_ETHERCAT_PDU).collect(),
                }),
            ethercat_receive: Mutex::new([0; MAX_ETHERCAT_FRAME]),
        }
	}
	
	// shorthands to PDU commands
	// the slave address is actually packed and unpacked to actual commands again, but this is greatly shortening the code and the compiler should optimize that
	
	/// shorthand for PDU BRD command
	pub async fn brd<T: PduData>(&self, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Broadcast, address).await
	}
	/// shorthand for PDU BWR command
	pub async fn bwr<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Broadcast, address, data).await
	}
	/// shorthand for PDU BRW command
	pub async fn brw<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Broadcast, address, data).await
	}
	
	/// shorthand for PDU APRD command
	pub async fn aprd<T: PduData>(&self, slave: u16, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::AutoIncremented(slave), address).await
	}
	/// shorthand for PDU APWR command
	pub async fn apwr<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::AutoIncremented(slave), address, data).await
	}
	/// shorthand for PDU APRW command
	pub async fn aprw<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::AutoIncremented(slave), address, data).await
	}
	/// shorthand for PDU ARMW command
	pub async fn armw(&self) {todo!()}
	
	/// shorthand for PDU FPRD command
	pub async fn fprd<T: PduData>(&self, slave: u16, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Fixed(slave), address).await
	}
	/// shorthand for PDU FPWR command
	pub async fn fpwr<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Fixed(slave), address, data).await
	}
	/// shorthand for PDU FPRW command
	pub async fn fprw<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Fixed(slave), address, data).await
	}
	/// shorthand for PDU FRMW command
	pub async fn frmw(&self) {todo!()}
	
	/// shorthand for PDU LRD command
	pub async fn lrd<T: PduData>(&self, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Logical, address).await
	}
	/// shorthand for PDU LWR command
	pub async fn lwr<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Logical, address, data).await
	}
	/// shorthand for PDU LRW command
	pub async fn lrw<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Logical, address, data).await
	}
	
	/// maps to a PDU *RD command
	pub async fn read<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>) -> PduAnswer<T> {
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BRD,
            SlaveAddress::AutoIncremented(_) => PduCommand::APRD,
            SlaveAddress::Fixed(_) => PduCommand::FPRD,
            SlaveAddress::Logical => PduCommand::LRD,
            };
        let mut buffer = T::Packed::uninit();
        buffer.as_mut().fill(0);
        PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u32, &mut buffer.as_mut()[.. memory.len]).await,
			value: T::unpack(buffer.as_ref()).unwrap(),
			}
    }
	/// maps to a PDU *WR command
	pub async fn write<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<()> {
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BWR,
            SlaveAddress::AutoIncremented(_) => PduCommand::APWR,
            SlaveAddress::Fixed(_) => PduCommand::FPWR,
            SlaveAddress::Logical => PduCommand::LWR,
            };
        let mut buffer = T::Packed::uninit();
        data.pack(buffer.as_mut()).unwrap();
		PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u32, &mut buffer.as_mut()[.. memory.len]).await,
			value: (),
			}
	}
	/// maps to a PDU *RW command
	pub async fn exchange<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<T> {
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BRW,
            SlaveAddress::AutoIncremented(_) => PduCommand::APRW,
            SlaveAddress::Fixed(_) => PduCommand::FPRW,
            SlaveAddress::Logical => PduCommand::LRW,
            };
        let mut buffer = T::Packed::uninit();
        data.pack(buffer.as_mut()).unwrap();
        PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u32, &mut buffer.as_mut()[.. memory.len]).await,
			value: T::unpack(buffer.as_ref()).unwrap(),
			}
	}
	
	/**
        Send a PDU on the ethercat bus.
        Returns the number of slaves who processed the command
	
        - the PDU is buffered with more PDUs if possible
        
        ### Parameters
        
        - `slave`: identifies what memory is accessed by this PDU (might even not be a slave memory, but a memory belonging to the whole segment)
        - `memory`: address in the selected memory
            + if slaves physical memory is accessed, it must be a 16bit address
            + if segment logical memory is accessed, it must be a 32bit address
        - `data`: buffer of data to send, and to write with the segment's answer, the answer will answer with the same data size as what was sent so the whole buffer will be sent and written back
    */
	pub async fn pdu(&self, command: PduCommand, slave: SlaveAddress, memory: u32, data: &mut [u8]) -> u16 {
        // assemble the address block with slave and memory addresses
        let address = match slave {
            SlaveAddress::Broadcast => u32::from(PhysicalAddress::new(
                0, 
                memory as u16,
                )),
            SlaveAddress::AutoIncremented(slave) => u32::from(PhysicalAddress::new(
                0u16.wrapping_sub(slave), 
                memory as u16,
                )),
            SlaveAddress::Fixed(slave) => u32::from(PhysicalAddress::new(
                slave, 
                memory as u16,
                )),
            SlaveAddress::Logical => memory,
        };
        
        let (token, ready, _finisher);
        loop {
            // buffering the pdu sending
            
            // this weird scope is here to prevent the rust thread checker to set this async future `!Send` just because there is remaining freed variables with `MutexGuard` type
            // TODO: restore the previous code (more readable and flexible) once https://github.com/rust-lang/rust/issues/104883 is fixed
            {
                let mut state = self.pdu_state.lock().unwrap();
                let space_available = || self.socket.max_frame() > state.last_end + data.len() + PduHeader::packed_size() + PduFooter::packed_size();
                let token_available = || ! state.free.is_empty();
                if ! token_available()  {
                    // there is nothing to do except waiting
                }
                else if ! space_available()  {
                    // sending the current buffer
                    assert!(self.socket.max_frame() > 
                        EthercatHeader::packed_size() 
                        + data.len() 
                        + PduHeader::packed_size() 
                        + PduFooter::packed_size(), "data too big for an ethercat frame");
                    state.ready = true;
                    self.sendable.notify_one();
                }
                else {
                    // reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
                    token = state.free.pop().unwrap();
                    state.receive[token] = Some(PduStorage {
                        // cast lifetime as static
                        // memory safety: this slice is pinned by the caller and its access is managed by field `ready`
                        data: unsafe {std::slice::from_raw_parts_mut(
                                data.as_mut_ptr(),
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
                        state.last_end = state.last_start;
                    }
                    
                    // stacking the PDU in self.pdu_receive
                    let advance = {
                        let range = state.last_end ..;
                        let mut cursor = Cursor::new(&mut state.send[range]);
                        cursor.pack(&PduHeader::new(
                            u8::from(command),
                            token as u8,
                            address,
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
                
                    // memory safety: this item in the array cannot be moved since self is borrowed, and will only be removed later by the current function
                    // we will access it potentially concurrently, but since we only want to detect a change in the value, that's fine
                    ready = unsafe {&*(&state.receive[token].as_ref().unwrap().ready as *const bool)};
                    // clean up the receive table at function end, or in case the async runtime cancels this task
                    _finisher = Finisher::new(|| {
                        let mut state = self.pdu_state.lock().unwrap();
                        state.receive[token] = None;
                        state.free.push(token).unwrap();
                    });
                    
                    break
                }
            }
            self.received.notified().await;
        }

        // waiting for the answer
        loop { 
            let notification = self.received.notified();
            if *ready {break}
            notification.await; 
        }
        
        {
            // free the token
            let state = self.pdu_state.lock().unwrap();
            state.receive[token].as_ref().unwrap().answers
        }
	}
	
	/// trigger sending the buffered PDUs, they will be sent as soon as possible by [Self::send] instead of waiting for the frame to be full or for the timeout
	pub fn flush(&self) {
        let mut state = self.pdu_state.lock().unwrap();
        state.ready = true;
        self.sendable.notify_one();
	}
	
	/// extract a received frame of PDUs and buffer each for reception by an eventual `self.pdu()` future waiting for it.
	fn pdu_receive(&self, state: &mut PduState, frame: &[u8]) {
        let mut frame = Cursor::new(frame);
        loop {
            let header = frame.unpack::<PduHeader>().unwrap();
            
            let token = usize::from(header.token());
            assert!(token <= state.receive.len());
            if let Some(mut storage) = state.receive[token].as_mut() {
                let content = frame.read(usize::from(u16::from(header.len()))).unwrap();
                
                // copy the PDU content in the reception buffer
                // concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                storage.data.copy_from_slice(content);
                let footer = frame.unpack::<PduFooter>().unwrap();
				storage.answers = footer.value; //footer.working_count();
				storage.ready = true;
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
        let mut state = self.pdu_state.lock().unwrap();
        // wait indefinitely if no data to send
        while state.last_end == 0
            {state = self.sendable.wait(state).unwrap();}
        // wait for more data until a timeout once data is present
        if ! state.ready
            {state = self.sendable.wait_timeout_while(
                    state, 
                    self.pdu_merge_time, 
                    |state| ! state.ready,
                    ).unwrap().0;}
        
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
        state.ready = false;
        state.last_end = 0;
        state.last_start = EthercatHeader::packed_size();
        self.sent.notify_waiters();
	}
}


/// dynamically specifies a destination address on the ethercat loop
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
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

/// container for a PDU command's answer
pub struct PduAnswer<T> {
    /// number of slaves who executed the command
	pub answers: u16,
	/// received value (will be the same as the sent value if no slave executed the command)
	pub value: T,
}
impl<T> PduAnswer<T> {
    /// extract the value only if exactly one slave answered
    pub fn one(self) -> T {
        self.exact(1)
    }
    /// extract the value only if the given amount of slaves answered
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
    /// address on the bus, its content depends on the command: [PhysicalAddress] or [u32]
    address: u32,
    /// data length following the header, excluding the footer. starting from `memory_address` in the addressed memory
    len: u11,
    reserved: u3,
    circulating: bool,
    /// true if there is an other PDU in the same PDU frame
    next: bool,
    interrupt: u16,
}
data::bilge_pdudata!(PduHeader, u80);

/// possible layout for [PduHeader::address]
#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Hash)]
struct PhysicalAddress {
    /// slave address in the ethercat segment
    slave: u16,
    /// address accessed in the slave's memory
    memory: u16,
}

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



