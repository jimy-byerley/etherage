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

use crate::{
    socket::*,
    data::{self, Field, PduData, ByteArray, PackingResult},
    };

const MAX_ETHERCAT_FRAME: usize = 2050;

/**
    low level ethercat functions, with no compile-time checking of the communication states
    this struct has no notion of slave, it is just executing basic commands
    
    genericity allows to use a UDP socket or raw ethernet socket
*/
pub struct RawMaster {
	/// (µs) acceptable delay time before sending buffered PDUs
	pdu_merge_time: Duration,
	
	// socket implementation
	socket: Box<dyn EthercatSocket>,
	// synchronization signal for multitask reception
	received: Notify,
	sendable: Condvar,
	send: Condvar,
	sent: Condvar,
	
	// communication state
    // states are locked using [std::sync::Mutex] since it is recommended by async-io
    // they should not be held for too long (and never during blocking operations) so they shouldn't disturb the async runtime too much
    
	pdu_state: Mutex<PduState>,
	ethercat_send: Mutex<heapless::Vec<u8, MAX_ETHERCAT_FRAME>>,
	ethercat_receive: Mutex<[u8; MAX_ETHERCAT_FRAME]>,
}
struct PduState {
	pub token: u8,
	pub last_start: usize,
	pub last_time: Instant,
	pub send: Vec<u8>,
	pub receive: HashMap<u8, PduStorage>,
}
/// struct for internal use in RawMaster
struct PduStorage {
    data: &'static [u8],
    ready: bool,
    answers: u16,
}
impl RawMaster {
	pub fn new<S: EthercatSocket>(socket: S) -> Self {        
        Self {
            pdu_merge_time: std::time::Duration::from_micros(2000), // microseconds
            
            socket: Box::new(socket),
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
            ethercat_send: Mutex::new(Default::default()),
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
	
	pub async fn aprd<T: PduData>(&self, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::AutoIncremented, address).await
	}
	pub async fn apwr<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::AutoIncremented, address, data).await
	}
	pub async fn aprw<T: PduData>(&self, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::AutoIncremented, address, data).await
	}
	pub async fn armw(&self) {todo!()}
	
	pub async fn fprd<T: PduData>(&self, slave: u16, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Configured(slave), address).await
	}
	pub async fn fpwr<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Configured(slave), address, data).await
	}
	pub async fn fprw<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Configured(slave), address, data).await
	}
	pub async fn frmw(&self) {todo!()}
	
	pub async fn lrd<T: PduData>(&self, slave: u16, address: Field<T>) -> PduAnswer<T> {
        self.read(SlaveAddress::Logical, address).await
	}
	pub async fn lwr<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<()> {
        self.write(SlaveAddress::Logical, address, data).await
	}
	pub async fn lrw<T: PduData>(&self, slave: u16, address: Field<T>, data: T) -> PduAnswer<T> {
        self.exchange(SlaveAddress::Logical, address, data).await
	}
	
	/// maps to a *rd command
	pub async fn read<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>) -> PduAnswer<T> {
        let (command, slave) = match slave {
            SlaveAddress::Broadcast => (PduCommand::BRD, 0),
            SlaveAddress::AutoIncremented => (PduCommand::APRD, 0),
            SlaveAddress::Configured(address) => (PduCommand::FPRD, address),
            SlaveAddress::Logical => (PduCommand::LRD, 0),
            };
        let mut data = T::ByteArray::new(0);
        PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u16, &mut data.as_mut_bytes_slice()[.. memory.len]).await,
			value: T::unpack(data.as_bytes_slice()).unwrap(),
			}
    }
	/// maps to a *wr command
	pub async fn write<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<()> {
        let (command, slave) = match slave {
            SlaveAddress::Broadcast => (PduCommand::BWR, 0),
            SlaveAddress::AutoIncremented => (PduCommand::APWR, 0),
            SlaveAddress::Configured(address) => (PduCommand::FPWR, address),
            SlaveAddress::Logical => (PduCommand::LWR, 0),
            };
		PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u16, &mut data.pack().as_mut_bytes_slice()[.. memory.len]).await,
			value: (),
			}
	}
	/// maps to a *rw command
	pub async fn exchange<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<T> {
        let (command, slave) = match slave {
            SlaveAddress::Broadcast => (PduCommand::BRW, 0),
            SlaveAddress::AutoIncremented => (PduCommand::APRW, 0),
            SlaveAddress::Configured(address) => (PduCommand::FPRW, address),
            SlaveAddress::Logical => (PduCommand::LRW, 0),
            };
        let mut data = data.pack();
        PduAnswer {
			answers: self.pdu(command, slave, memory.byte as u16, &mut data.as_mut_bytes_slice()[.. memory.len]).await,
			value: T::unpack(data.as_bytes_slice()).unwrap(),
			}
	}
	
	/// send a PDU on the ethercat bus
	/// the PDU is buffered with more PDUs if possible
	/// returns the number of slaves who processed the command
	pub async fn pdu(&self, command: PduCommand, slave_address: u16, memory_address: u16, data: &mut [u8]) -> u16 {
        let token;
        let (ready, finisher) = {
            // buffering the pdu sending
            let mut state = self.pdu_state.lock().unwrap();
            
            // sending the buffer if necessary
            while MAX_ETHERCAT_FRAME < state.send.len() + data.len() + PduHeader::packed_size() {
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
            if state.last_start < state.send.len() {
                let start = state.last_start;
                let place = &mut state.send[start ..][.. PduHeader::packed_size()];
                let mut header = PduHeader::unpack(place).unwrap();
                header.set_next(true);
                place.copy_from_slice(&header.pack());
            }
            else {
                state.last_time = Instant::now();
            }
            
            // stacking the PDU in self.pdu_receive
            state.last_start = state.send.len();
            state.send.extend_from_slice(PduHeader::new(
                command as u8,
                token,
                slave_address,
                memory_address,
                u11::new(data.len().try_into().unwrap()),
                false,
                false,
                0,
                ).pack().as_bytes_slice());
            state.send.extend_from_slice(data);
            state.send.extend_from_slice(PduFooter::new(0)
                .pack().as_bytes_slice());
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
        
		let mut state = self.pdu_state.lock().unwrap();
		state.receive[&token].answers
	}
	
	/// extract a received frame of PDUs and buffer each for reception by an eventual `self.pdu()` future waiting for it.
	fn pdu_receive(&self, state: &mut PduState, frame: &[u8]) {
        let mut frame = frame;
        loop {
            let header = PduHeader::unpack(&frame).unwrap();
            if let Some(storage) = state.receive.get(&header.token()) {
                let content = &frame[PduHeader::packed_size() ..];
                let content = &content[.. header.len().value() as usize];
                let footer = PduFooter::unpack(&content[header.len().value() as usize ..]).unwrap();
                
                // copy the PDU content in the reception buffer
                // concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                unsafe {std::slice::from_raw_parts_mut(
                    storage.data.as_ptr() as *mut u8,
                    storage.data.len(),
                    )}
                    .copy_from_slice(content);
                let storage = unsafe {&mut *(storage as *const PduStorage as *mut PduStorage)};
				storage.ready = true;
				storage.answers = footer.working_count();
            }
            frame = &frame[
                PduHeader::packed_size() 
                + PduFooter::packed_size() 
                + header.len().value() as usize 
                ..];
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
        
        let mut send = self.ethercat_send.lock().unwrap();
        // send
        send.extend_from_slice(EthercatHeader::new(
            u11::new(state.send.len() as u16),
            EthercatType::PDU,
            ).pack().as_bytes_slice()).unwrap();
        send.extend_from_slice(&state.send).unwrap();
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
pub enum SlaveAddress {
	/// every slave will receive and execute
	Broadcast,
	/// address will be determined by the topology (index of the slave in the ethernet loop)
	AutoIncremented,
	/// address has been set by the master previously
	Configured(u16),
	/// the logical memory is the destination, all slaves are concerned
	Logical,
}

pub struct PduAnswer<T> {
	pub answers: u16,
	pub value: T,
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
data::bilge_pdudata!(EthercatHeader);

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


// this is to see if the brute frame assembling currently made in RawMaster could be made in a packing/unpacking scheme like for ethernet frames
// use std::io::{Cursor, Write};
// 
// struct PduFrames<T> {
//     data: T,
//     last: usize,
//     position: usize,
// }
// impl<'a, T: AsRef<[u8]>> PduFrames<T> {
//     fn read(&'a mut self) -> PduFrame<'a> {
//         let frame = PduFrame::unpack(&self.data.as_ref()[self.position ..]);
//         self.position += frame.size();
//         frame
//     }
// }
// impl<T: AsMut<[u8]>> PduFrames<T> {
//     fn write(&mut self, frame: PduFrame<'_>) {
//         if self.last < self.position {
//             PduFrame::unpack(&self.data.as_mut()[self.last ..]).header.set_next(true)
//         }
//         frame.pack(&mut self.data.as_mut()[self.position ..]);
//         self.position += frame.size();
//     }
// }
// 
// struct PduFrame<'a> {
//     header: PduHeader,
//     data: &'a [u8],
// }
// impl<'a> PduFrame<'a> {
//     fn size(&self) -> usize {0}
//     fn pack(&self, dst: &mut [u8]) {
//         let mut dst = Cursor::new(dst);
//         dst.write(self.header.pack().unwrap().as_bytes_slice()).unwrap();
//         dst.write(self.data).unwrap();
//     }
//     fn unpack(src: &'a [u8]) -> Self {
//         let header = PduHeader::unpack_from_slice(
//             &src[.. <PduHeader as PackedStruct>::packed_size()]
//             ).unwrap();
//         let data = &src[<PduHeader as PackedStruct>::packed_size() ..];
//         let data = &data[.. data.len() - <PduFooter as PackedStruct>::packed_size()];
//         Self{header, data}
//     }
// }

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
    /// data length starting from `memory_address`
    len: u11,
    reserved: u3,
    circulating: bool,
    /// true if there is an other PDU in the same PDU frame
    next: bool,
    interrupt: u16,
}
data::bilge_pdudata!(PduHeader);

/// footer for PDU exchange
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Default)]
struct PduFooter {
    working_count: u16,
}
data::bilge_pdudata!(PduFooter);

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



