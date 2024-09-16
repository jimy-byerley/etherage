/*!
	low level ethercat communication functions.

	It wraps an ethercat socket to schedule, send and receive ethercat frames containing data or commands.
*/

use std::{
    sync::{Arc, Mutex},
    time::Instant,
    };
use core::{
    ops::DerefMut,
    time::Duration,
    sync::atomic::AtomicU16,
    sync::atomic::Ordering::*,
    };
use tokio::{
    task::JoinHandle,
    sync::Notify,
    };
use tokio_timerfd::{Delay, Interval};
use bilge::prelude::*;
use futures_concurrency::future::Race;
use futures::StreamExt;
use core::future::poll_fn;

use crate::{
    error::{EthercatError, EthercatResult},
    socket::*,
    data::{self, Field, PduData, Storage, Cursor, PackingResult},
    registers::ExternalEvent,
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

// trait EthercatSocket: crate::socket::EthercatSocket + AsRawFd {}

/**
    low level ethercat communication functions, with no notion of slave.
    
    genericity allows to use a UDP socket or raw ethernet socket, see [crate::socket] for more details.
    
    This struct does not do any compile-time checking of the communication states on the slaves, and has no notion of slave, it is just executing the basic commands.
    
    The ethercat low level is all about PDUs: an ethercat frame intended for slaves is a PDU frame. PDU frames contain any number of PDU (Process Data Unit), each PDU is a command, acting on one of the 2 memories types:
    
    - **Physical Memory** (aka. registers)
    
        each slave has its own physical memory, commands for physical memory (`*P*`, `B*`) are addressing a specific slave, or combining the memory reads from all slaves

        The physical memory is divided into registers declared in [crate::registers]

    - **Logical Memory** (aka. fieldbus memory)

        this memory doesn't physically exist anywhere, but can be read/write using `L*`  commands with each slave contributing to the record according to the configuration set before.

        The logical memory is organized by the mapping set in the FMMU (Fieldbust Memory Management Unit). [crate::mapping] helps on this task.

    See variants of [PduCommand] and [Self::pdu] for more details.

    The following scheme shows an overview of the features and memory areas of every ethercat slave. Memory copy operations are represented as plain arrows regardless of the real sequence of commands needed to perform the operation. *RT* flag marks what can be acheived in realtime, and what can not.

	![ethercat sub protocols](https://raw.githubusercontent.com/jimy-byerley/etherage/master/schemes/ethercat-protocols.svg)
*/

pub struct RawMaster {
    /// (µs) acceptable delay time before sending buffered PDUs
    pdu_merge_time: Duration,
    /// delay since the PDU sending before aborting its reception (and dropping the token)
    pdu_timeout: Duration,

    /// socket implementation
    socket: Box<dyn EthercatSocket + Send + Sync>,
    /// spawned async tasks
    task: Mutex<Option<JoinHandle<()>>>,
    /// synchronization signal for multitask reception
    received: Notify,
    sendable: Notify,
    sent: Notify,

    /**
        communication state

        states are locked using [std::sync::Mutex] since it is recommended by async-io
        they should not be held for too long (and never during blocking operations) so they shouldn't disturb the async runtime too much
    */
    state: Mutex<MasterState>,
}
struct MasterState {
    last_start: usize,
    last_end: usize,
    ready: bool,
    /// send buffer, it contains one ethercat frame
    send: [u8; MAX_ETHERCAT_FRAME],
    /// reception destination, each containing a reception buffer and additional infos
    receive: [Option<PduState>; 2*MAX_ETHERCAT_PDU],
    /// list of free reception storages, indices in [receive]
    free: heapless::Vec<usize, {2*MAX_ETHERCAT_PDU}>,

    buffered: Instant,
}
/// struct for internal use in RawMaster
struct PduState {
    // allocated buffer for reception
    data: &'static mut [u8],
    // frame working count + 1, staying zero until a frame is received or timeout
    ready: AtomicU16,
    // sending instant, allowing to count a timeout
    sent: Instant,
}
impl RawMaster {
    pub fn new<S: EthercatSocket + 'static + Send + Sync>(socket: S) -> Arc<Self> {
        let master = Arc::new(Self {
            pdu_merge_time: Duration::from_micros(100),
            pdu_timeout: Duration::from_millis(100),

            socket: Box::new(socket),
            received: Notify::new(),
            sendable: Notify::new(),
            sent: Notify::new(),

            state: Mutex::new(MasterState {
                last_start: EthercatHeader::packed_size(),
                last_end: 0,
                ready: false,
                send: [0; MAX_ETHERCAT_FRAME],
                receive: [0; 2*MAX_ETHERCAT_PDU].map(|_| None),
                free: (0 .. 2*MAX_ETHERCAT_PDU).collect(),

                buffered: Instant::now(),
                }),
            task: Mutex::new(None),
        });
        master.task.lock().unwrap().replace(tokio::task::spawn({
            let master = master.clone();
            async move { master.task_loop().await; }
        }));
        master
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
        let mut buffer = T::Packed::uninit();
        buffer.as_mut().fill(0);
        PduAnswer {
            answers: self.read_slice(slave, memory.byte as _, &mut buffer.as_mut()[.. memory.len]).await.answers,
            data: buffer,
            }
    }
    /// maps to a PDU *WR command
    pub async fn write<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<()> {
        let mut buffer = T::Packed::uninit();
        data.pack(buffer.as_mut()).unwrap();
        self.write_slice(slave, memory.byte as _, &mut buffer.as_mut()[.. memory.len]).await
    }
    /// maps to a PDU *RW command
    pub async fn exchange<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<T> {
        let mut buffer = T::Packed::uninit();
        data.pack(buffer.as_mut()).unwrap();
        PduAnswer {
            answers: self.exchange_slice(slave, memory.byte as _, &mut buffer.as_mut()[.. memory.len]).await.answers,
            data: buffer,
            }
    }
    /// maps to a PDU *RMW
    pub async fn multiple<T: PduData>(&self, slave: SlaveAddress, memory: Field<T>, data: T) -> PduAnswer<T> {
		let command = match slave {
			SlaveAddress::AutoIncremented(_) => PduCommand::ARMW,
			SlaveAddress::Fixed(_) => PduCommand::FRMW,
			_ => unimplemented!("read-multiple-write can only be used with a specific slave for reading"),
			};
		let mut buffer = T::Packed::uninit();
		data.pack(buffer.as_mut()).unwrap();
        let answers = self.topic(command, slave, memory.byte as _, &mut buffer.as_mut()[.. memory.len]).await
                        .send(None).await
                        .wait().await
                        .receive(None).answers;
		PduAnswer {
            answers,
            data: buffer,
			}
    }

    /// maps to a PDU *RD command
    pub async fn read_slice(&self, slave: SlaveAddress, memory: u32, data: &mut [u8]) -> PduAnswer<()> {
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BRD,
            SlaveAddress::AutoIncremented(_) => PduCommand::APRD,
            SlaveAddress::Fixed(_) => PduCommand::FPRD,
            SlaveAddress::Logical => PduCommand::LRD,
            };
        self.topic(command, slave, memory, data).await
                        .send(None).await
                        .wait().await
                        .receive(None)
    }
    /// maps to a PDU *WR command
    pub async fn write_slice(&self, slave: SlaveAddress, memory: u32, data: &mut [u8]) -> PduAnswer<()> {
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BWR,
            SlaveAddress::AutoIncremented(_) => PduCommand::APWR,
            SlaveAddress::Fixed(_) => PduCommand::FPWR,
            SlaveAddress::Logical => PduCommand::LWR,
            };
        self.topic(command, slave, memory, data).await
                        .send(None).await
                        .wait().await
                        .receive(None)
    }
    /// maps to a PDU *RW command
    pub async fn exchange_slice(&self, slave: SlaveAddress, memory: u32, data: &mut [u8]) -> PduAnswer<()> {
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BRW,
            SlaveAddress::AutoIncremented(_) => PduCommand::APRW,
            SlaveAddress::Fixed(_) => PduCommand::FPRW,
            SlaveAddress::Logical => PduCommand::LRW,
            };
        self.topic(command, slave, memory, data).await
                        .send(None).await
                        .wait().await
                        .receive(None)
    }

    /**
        reserve a token (a PDU index) for sending PDUs through the ethercat segment.
        This is the low-level layer of all other PDU sending methods

        ### Parameters

        - `slave`: identifies what memory is accessed by this PDU (might even not be a slave memory, but a memory belonging to the whole segment)
        - `memory`: address in the selected memory
            + if slaves physical memory is accessed, it must be a 16bit address
            + if segment logical memory is accessed, it must be a 32bit address
        - `data`: buffer of data to send, and to write with the segment's answer, the answer will answer with the same data size as what was sent so the whole buffer will be sent and written back
        - `flush`: all PDUs in the buffer will be sent together with this PDU immediately rather than withing for the buffering delay, this is helpful for time-bound applications
    */
    pub async fn topic<'a>(&'a self, command: PduCommand, slave: SlaveAddress, memory: u32, buffer: &'a mut [u8]) -> Topic<'a> {
        Topic::new(self, command, slave, memory, buffer).await
    }

    /** 
        trigger sending the buffered PDUs, they will be sent as soon as possible by the sending task instead of waiting for the frame to be full or for the timeout
        
        Note: this method is helpful to manage the stream and make space in the buffer before sending PDUs, but does not help to make sure a PDU is sent deterministic time. To trigger the sending of a PDU, use argument `flush` of [Self::pdu]
    */
    pub fn flush(&self) {
        let mut state = self.state.lock().unwrap();
        if state.last_end != 0 {
            state.ready = true;
            self.sendable.notify_one();
        }
    }

    /// extract a received frame of PDUs and buffer each for reception by an eventual `self.pdu()` future waiting for it.
    fn pdu_receive(&self, state: &mut MasterState, frame: &[u8]) -> EthercatResult<()> {
        let _finisher = Finisher::new(|| self.received.notify_waiters());
        let mut frame = Cursor::new(frame);
        loop {
            let header = frame.unpack::<PduHeader>()
                            .map_err(|_| EthercatError::Protocol("unable to unpack PDU header, skiping all remaning PDUs in frame"))?;
            
            let token = usize::from(header.token());
            if token >= state.receive.len()
                {return Err(EthercatError::Protocol("received inconsistent PDU token"))}
            
            if let Some(storage) = state.receive[token].as_mut() {
                let content = frame.read(usize::from(u16::from(header.len())))
                    .map_err(|_|  EthercatError::Protocol("PDU size mismatch"))?;
                
                // copy the PDU content in the reception buffer
                // concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                storage.data.copy_from_slice(content);
                let footer = frame.unpack::<PduFooter>()
                    .map_err(|_|  EthercatError::Protocol("unable to unpack PDU footer"))?;
                storage.ready.store(footer.working_count().saturating_add(1), Relaxed);
            }
            if ! header.next() {break}
            if frame.remain().len() == 0 
                {return Err(EthercatError::Protocol("inconsistent ethercat frame size: remaining unused data after PDUs"))}
        }
        Ok(())
    }
    
    /**
        this is the socket reception handler
        
        it receives and process one datagram, it may be called in loop with no particular timer since the sockets are assumed blocking
        
        In case something goes wrong during PDUs unpacking, the PDUs successfully processed will be reported to their pending futures, the futures for PDU which caused the read to fail and all following PDUs in the frame will be left pending (since the data is corrupted, there is no way to determine which future should be aborted)
    */
    async fn task_receive(&self) -> EthercatResult {
        let mut receive = [0; MAX_ETHERCAT_FRAME];
        loop {
            let size = poll_fn(|cx|  self.socket.poll_receive(cx, &mut receive) ).await?;
            let mut frame = Cursor::new(&receive[.. size]);
            
            let header = frame.unpack::<EthercatHeader>()?;
            if frame.remain().len() < header.len().value() as usize
                {return Err(EthercatError::Protocol("received frame header has inconsistent length"))}
            let content = &frame.remain()[.. header.len().value() as usize];
            
            assert!(header.len().value() as usize <= content.len());
            match header.ty() {
                EthercatType::PDU => self.pdu_receive(
                                        self.state.lock().unwrap().deref_mut(),
                                        content,
                                        )?,
                // what is this ?
                EthercatType::NetworkVariable => todo!(),
                // no mailbox frame shall transit to this master, ignore it or raise an error ?
                EthercatType::Mailbox => {},
            }
        }
    }
    /// this is the socket sending handler
    async fn task_send(&self) -> EthercatResult {
        let mut delay = Delay::new(Instant::now())?;
        loop {
            let delay = &mut delay;
            
            // wait indefinitely if no data to send
            let ready = loop {
                self.sendable.notified().await;
                let state = self.state.lock().unwrap();
                if state.last_end != 0  {break state.ready}
            };
            
            let send = {
                let mut state = if ready {
                    self.state.lock().unwrap()
                }
                else {
                    delay.reset(Instant::now() + self.pdu_merge_time);
                    // wait for more data until a timeout once data is present
                    (
                        // timeout for sending the batch
                        async {
                            delay.await.unwrap();
                            // TODO: handle the possible ioerror in the delay
                            self.state.lock().unwrap()
                        },
                        // wait for more data in the batch
                        async { loop {
                            self.sendable.notified().await;
                            let state = self.state.lock().unwrap();
                            if state.ready  {break state}
                        }},
                    ).race().await
                };
                state.ready = true;
                
                // check header
                EthercatHeader::new(
                    u11::new((state.last_end - EthercatHeader::packed_size()) as u16),
                    EthercatType::PDU,
                    ).pack(&mut state.send).unwrap();

                unsafe {std::slice::from_raw_parts_mut(
                                state.send.as_mut_ptr(), 
                                state.last_end,
                                )}
            };
            
            // send
            poll_fn(|cx| self.socket.poll_send(cx, &send) ).await?;
            {
                let mut state = self.state.lock().unwrap();

                // reset state
                state.ready = false;
                state.last_end = 0;
                state.last_start = EthercatHeader::packed_size();
            }
            self.sent.notify_waiters();
        }
    }
    
    async fn task_timeout(&self) -> EthercatResult {
        let mut delay = Interval::new_interval(self.pdu_timeout)?;
        loop {
            delay.next().await.unwrap().unwrap();
            let mut state = self.state.lock().unwrap();
            let date = Instant::now();
            
            for (token, storage) in state.receive.iter_mut().enumerate() {
                if let Some(storage) = storage {
                    if date.duration_since(storage.sent) > self.pdu_timeout {
                        println!("token {} timeout", token);
                        storage.ready.store(1, Relaxed);
                    }
                }
            }
            self.received.notify_waiters();
        }
    }
    
    async fn task_loop(&self) {
        (
            async { self.task_receive().await.unwrap() },
            async { self.task_send().await.unwrap() },
            async { self.task_timeout().await.unwrap() },
        ).race().await
    }
    
    pub fn stop(&self) {
        self.task.lock().unwrap().as_ref().map(|handle|  handle.abort());
    }
}

pub struct Topic<'a> {
    master: &'a RawMaster,
    ready: &'a AtomicU16,
    token: usize,
    command: PduCommand,
    target: u32,
}
impl<'a> Topic<'a> {
    pub async fn new(master: &'a RawMaster, command: PduCommand, slave: SlaveAddress, memory: u32, buffer: &'a mut [u8]) -> Topic<'a> {
        // assemble the address block with slave and memory addresses
        let target = match slave {
            SlaveAddress::Broadcast => u32::from(MemoryAddress::new(
                0,
                memory as u16,
                )),
            SlaveAddress::AutoIncremented(slave) => u32::from(MemoryAddress::new(
                0u16.wrapping_sub(slave),
                memory as u16,
                )),
            SlaveAddress::Fixed(slave) => u32::from(MemoryAddress::new(
                slave,
                memory as u16,
                )),
            SlaveAddress::Logical => memory,
        };

        let (token, ready);
        loop {
            {
                let mut state = master.state.lock().unwrap();
                if state.free.is_empty() {
                    // there is nothing to do except waiting
                    master.received.notified()
                }
                else {
                    // reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
                    token = state.free.pop().unwrap();

                    state.receive[token] = Some(PduState {
                        sent: Instant::now(),
                        // cast lifetime as static
                        // memory safety: this slice is pinned by the caller and its access is managed by field `ready`
                        data: unsafe {std::slice::from_raw_parts_mut(
                                buffer.as_mut_ptr(),
                                buffer.len(),
                                )},
                        ready: AtomicU16::new(0),
                        });

                    // memory safety: this item in the array cannot be moved since self is borrowed, and will only be removed later by the current function
                    // we will access it potentially concurrently, but since we only want to detect a change in the value, that's fine
                    ready = unsafe {&*(&state.receive[token].as_ref().unwrap().ready as *const AtomicU16)};

                    break
                }
            }.await;
        }

        Topic {
            master,
            ready,
            token,
            command,
            target,
            }
    }
    /// send the given data in a pdu
    pub async fn send(&mut self, data: Option<&[u8]>) -> &'_ Self {
        loop {
            // this weird scope is here to prevent the rust thread checker to set this async future `!Send` just because there is remaining freed variables with `MutexGuard` type
            {
                let mut state = self.master.state.lock().unwrap();
                // borrowing the data buffer: safety dude, we are only using this field in this scope, the borrow checker should allow that
                let data = data.unwrap_or(state.receive[self.token].as_ref().unwrap().data);
                let data = unsafe {std::slice::from_raw_parts(
                        data.as_ptr(),
                        data.len(),
                        )};

                if state.ready {
                    self.master.sent.notified()
                }
                else if ! (self.master.socket.max_frame() > state.last_end.max(state.last_start)
                            + data.len()
                            + PduHeader::packed_size()
                            + PduFooter::packed_size())  {
                    // sending the current buffer
                    state.ready = true;
                    self.master.sendable.notify_one();
                    self.master.sent.notified()
                }
                else {
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

                    // stacking the PDU
                    let advance = {
                        let range = state.last_end ..;
                        let mut cursor = Cursor::new(&mut state.send[range]);
                        cursor.pack(&PduHeader::new(
                            self.command,
                            self.token as u8,
                            self.target,
                            u11::new(data.len().try_into().unwrap()),
                            false,
                            false,
                            ExternalEvent::default(),
                            )).unwrap();
                        cursor.write(data).unwrap();
                        cursor.pack(&PduFooter::new(0)).unwrap();
                        cursor.position()
                    };
                    state.last_start = state.last_end;
                    state.last_end = state.last_start + advance;
                    state.receive[self.token].as_mut().unwrap().sent = Instant::now();

                    state.buffered = Instant::now();
//                     println!("2-bufferized {} {}",
//                             SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
//                             self.token);

                    self.master.sendable.notify_one();
                    break
                }
            }.await;
        }
        self
    }
    /// return true if the answer is available
    pub fn available(&self) -> bool {
        self.ready.load(SeqCst) != 0
    }
    /// waiting for the answer
    pub async fn wait(&self) -> &'_ Self {
        loop {
            let notification = self.master.received.notified();
            if self.available() {break}
            notification.await;
        }
        self
    }
    /// copy the received data in the given buffer
    pub fn receive(&self, data: Option<&mut [u8]>) -> PduAnswer<()> {
        let answers = if let Some(data) = data {
            let state = self.master.state.lock().unwrap();
            let storage = state.receive[self.token].as_ref().unwrap();

            data.copy_from_slice(storage.data);
            self.ready.swap(0, Relaxed).saturating_sub(1)
        }
        else {
            self.ready.swap(0, Relaxed).saturating_sub(1)
        };
//         {
//             let state = self.master.state.lock().unwrap();
//             let storage = state.receive[self.token].as_ref().unwrap();

//             println!("7-pdu {} {}",
//                 SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
//                 storage.sent.elapsed().as_nanos());
//         }
//         println!("8-token {} {}",
//             SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
//             self.token);
        PduAnswer {
            answers,
            data: [],
            }
    }
}
impl Drop for Topic<'_> {
    fn drop(&mut self) {
        let mut state = self.master.state.lock().unwrap();
        // free the token
        if state.receive[self.token].is_some() {
            state.receive[self.token] = None;
            state.free.push(self.token).unwrap();
        }
        // BUG: freeing the token on future cancelation does not cancel the packet reception, so the received packet may interfere with any next PDU using this token
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
pub struct PduAnswer<T: PduData> {
    /// number of slaves who executed the command
    pub answers: u16,
    /// received value (will be the same as the sent value if no slave executed the command)
    pub data: T::Packed,
}
impl<T: PduData> PduAnswer<T> {
    /// extract the value only if exactly one slave answered
    pub fn one(self) -> EthercatResult<T> {
        self.exact(1)
    }
    /// extract the value only if the given amount of slaves answered
    pub fn exact(&self, n: u16) -> EthercatResult<T> {
        if self.answers != n {
            if self.answers == 0 
                {return Err(EthercatError::Protocol("no slave answered"))}
            else if self.answers < n
                {return Err(EthercatError::Protocol("to few slaves answered"))}
            else if self.answers > n
                {return Err(EthercatError::Protocol("to much slaves answered"))}
        }
        Ok(self.value()?)
    }
    /// extract the value if any slave answered
    pub fn any(&self) -> EthercatResult<T> {
        if self.answers == 0
            {return Err(EthercatError::Protocol("no slave answered"))}
        Ok(self.value()?)
    }
    /// extract the value, whatever it is and if slaves answered or not
    pub fn value(&self) -> PackingResult<T> {
        T::unpack(self.data.as_ref())
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
    command: PduCommand,
    /// PDU task request identifier
    token: u8,
    /// address on the bus, its content depends on the command: [MemoryAddress] or [u32]
    address: u32,
    /// data length following the header, excluding the footer. starting from `memory_address` in the addressed memory
    len: u11,
    reserved: u3,
    circulating: bool,
    /// true if there is an other PDU in the same PDU frame
    next: bool,
    interrupt: ExternalEvent,
}
data::bilge_pdudata!(PduHeader, u80);

/// possible layout for [PduHeader::address]
#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Hash)]
struct MemoryAddress {
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
    /// all slaves will read their physical memory and retransmit a bitwise-or of their local data with the received data
    BRD = 0x07,
    /// broadcast write
    /// all slaves will write their physical memory with the received data
    BWR = 0x08,
    /// broadcast read & write
    /// actually it is write-then-read
    BRW = 0x09,

    /// auto-incremented slave read
    /// the specified slave will read its physical memory and transmit it
    /// the slave is specified using its rank in the ethercat ring
    APRD = 0x01,
    /// auto-incremented slave write
    /// the specified slave will write its physical memory with the received data
    /// the slave is specified using its rank in the ethercat ring
    APWR = 0x02,
    /// auto-incremented slave read & write
    /// actually it is write-then-read
    /// the slave is specified using its rank in the ethercat ring
    APRW = 0x03,

    /// fixed slave read
    /// the specified slave will read its physical memory and transmit it
    /// the slave is specified using a previously fixed address
    FPRD = 0x04,
    /// fixed slave write
    /// the specified slave will write its physical memory with the received data
    /// the slave is specified using a previously fixed address
    FPWR = 0x05,
    /// fixed slave read & write
    FPRW = 0x06,

    /// logical memory read
    /// all slave will read their memory mapped using FMMUs enabled for reading, and complete the data received before transmiting
    LRD = 0x0A,
    /// logical memory write
    /// all slave will write their memory mapped using FMMUs enabled for writting
    LWR = 0x0B,
    /// logical memory read & write
    LRW = 0x0C,

    /// auto-incremented slave read multiple write
    /// the slave matching this PDU'address will read the data and retransmit, other slaves will write it
    ARMW = 0x0D,
    /// fixed slave read multiple write
    /// the slave matching this PDU'address will read the data and retransmit, other slaves will write it
    FRMW = 0x0E,
}


// callback that will be called on object drop
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
