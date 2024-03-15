/*!
	low level ethercat communication functions.

	It wraps an ethercat socket to schedule, send and receive ethercat frames containing data or commands.
*/

use std::{
    sync::Arc,
    time::Instant,
    };
use core::{
    ops::DerefMut,
    time::Duration,
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
use magetex::*;

use crate::{
    error::{EthercatError, EthercatResult},
    socket::*,
    data::{self, Field, PduData, Storage, Cursor, PackingResult},
    };
use std::ops::Deref;


/// maximum frame size, currently limited to the ethernet frame size
// size tolerated by its header (content size coded with 11 bits)
// const MAX_ETHERCAT_FRAME: usize = 2050;
// size tolerated by ethernet encapsulation
const MAX_ETHERCAT_FRAME: usize = 1500;
/// minimum PDU size
const MIN_PDU: usize = 60;
/// maximum number of PDU in an ethercat frame
const MAX_ETHERCAT_PDU: usize = MAX_ETHERCAT_FRAME / MIN_PDU;
/// time use for allowed on PDU merginging before timeout
const PDU_MERGE_TIME_DEFAULT : u64 = 100 * 10;

// trait EthercatSocket: crate::socket::EthercatSocket + AsRawFd {}

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
    sendable: Notify,
    sent: Notify,

    // communication state
    // states are locked using [std::sync::Mutex] since it is recommended by async-io
    // they should not be held for too long (and never during blocking operations) so they shouldn't disturb the async runtime too much

    pdu_state: Mutex<PduState>,
    task: Mutex<Option<JoinHandle<()>>>,
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
    sent: Instant,
}

impl RawMaster {
    pub fn new<S: EthercatSocket + 'static + Send + Sync>(socket: S) -> Arc<Self> {
        let master = Arc::new(Self {
            pdu_merge_time: std::time::Duration::from_micros(PDU_MERGE_TIME_DEFAULT),
            socket: Box::new(socket),
            received: Notify::new(),
            sendable: Notify::new(),
            sent: Notify::new(),

            pdu_state: Mutex::new(PduState {
                last_start: EthercatHeader::packed_size(),
                last_end: 0,
                ready: false,
                send: [0; MAX_ETHERCAT_FRAME],
                receive: [0; 2*MAX_ETHERCAT_PDU].map(|_| None),
                free: (0 .. 2*MAX_ETHERCAT_PDU).collect(),
                }),
            task: Mutex::new(None),
        });

        // Lock in synchrone because it's the Ctor of the master
        master.task.sync_lock().replace(tokio::task::spawn({
                let master = master.clone();
                async move { master.task_loop().await; }}));
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
        let command = match slave {
            SlaveAddress::Broadcast => PduCommand::BRD,
            SlaveAddress::AutoIncremented(_) => PduCommand::APRD,
            SlaveAddress::Fixed(_) => PduCommand::FPRD,
            SlaveAddress::Logical => PduCommand::LRD,
            };
        let mut buffer = T::Packed::uninit();
        buffer.as_mut().fill(0);
        PduAnswer {
            answers: self.pdu(command, slave, memory.byte as u32, &mut buffer.as_mut()[.. memory.len], false).await,
            data: buffer,
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
            answers: self.pdu(command, slave, memory.byte as u32, &mut buffer.as_mut()[.. memory.len], false).await,
            data: [],
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
            answers: self.pdu(command, slave, memory.byte as u32, &mut buffer.as_mut()[.. memory.len], false).await,
            data: buffer,
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
        - `flush`: all PDUs in the buffer will be sent together with this PDU immediately rather than withing for the buffering delay, this is helpful for time-bound applications
    */
    pub async fn pdu(&self, command: PduCommand, slave: SlaveAddress, memory: u32, data: &mut [u8], flush: bool) -> u16 {

        // assemble the address block with slave and memory addresses
        let address = match slave {
            SlaveAddress::Broadcast => u32::from(PhysicalAddress::new(
                0,
                memory as u16)),
            SlaveAddress::AutoIncremented(slave) => u32::from(PhysicalAddress::new(
                0u16.wrapping_sub(slave),
                memory as u16)),
            SlaveAddress::Fixed(slave) => u32::from(PhysicalAddress::new(
                slave,
                memory as u16)),
            SlaveAddress::Logical => memory,
        };

        // Buffering the pdu sending
        //println!("Pdu - buffering request");
        let (is_ready, token) = {
            let mut state = loop {
                let mut state = self.pdu_state.lock().await;
                //let space_available = || self.socket.max_frame() > state.last_end + data.len() + PduHeader::packed_size() + PduFooter::packed_size();
                let space_available = || state.last_end + data.len() + PduHeader::packed_size() + PduFooter::packed_size();

                if state.free.is_empty() {
                    // there is nothing to do except waiting
                    drop(state);
                    self.received.notified().await;
                }
                else if state.ready {
                    drop(state);
                    self.sent.notified().await;
                }
                else if self.socket.max_frame() <= space_available()  {
                    // Sending the current buffer
                    assert!(self.socket.max_frame() > space_available() - state.last_end, "data too big for an ethercat frame");
                    state.ready = true;
                    drop(state);
                    self.sendable.notify_one();
                    self.sent.notified().await;
                }
                else { break state; }
            };

            // Reserving a token number to ensure no other task will exchange a PDU with the same token and receive our data
            {
                let token = state.free.pop().unwrap();
                assert!(state.receive[token].is_none());
                state.receive[token] = Some(PduStorage {
                    // cast lifetime as static
                    // memory safety: this slice is pinned by the caller and its access is managed by field `ready`
                    data: unsafe {std::slice::from_raw_parts_mut(data.as_mut_ptr(), data.len())},
                    ready: false,
                    answers: 0,
                    sent: Instant::now(),
                });

                // Change last value's PduHeader.next
                if state.last_start <= state.last_end {
                    let range = state.last_start .. state.last_end;
                    let place = &mut state.send[range];
                    let mut header = PduHeader::unpack(place).unwrap();
                    header.set_next(true);
                    header.pack(place).unwrap();
                }
                else { state.last_end = state.last_start; }

                // Stacking the PDU
                let advance : usize = {
                    let range = state.last_end ..;
                    let length = u11::new(data.len().try_into().unwrap());
                    let mut cursor = Cursor::new(&mut state.send[range]);
                    cursor.pack(&PduHeader::new(command, token as u8, address, length, false, false, u16::new(0))).unwrap();
                    cursor.write(data).unwrap();
                    cursor.pack(&PduFooter::new(0)).unwrap();
                    cursor.position()
                };
                state.last_start = state.last_end;
                state.last_end = state.last_start + advance;
                state.ready = flush;

                // memory safety: this item in the array cannot be moved since self is borrowed, and will only be removed later by the current function
                // we will access it potentially concurrently, but since we only want to detect a change in the value, that's fine
                (unsafe {&*(&state.receive[token].as_ref().unwrap().ready as *const bool)}, token)
            }
        };
        let delay: Instant = Instant::now();

        // Clean up the receive table at function end, or in case the async runtime cancels this task
        let _finisher = Finally::new(|| {
            // free the token
            let mut fstate = self.pdu_state.sync_lock();
            fstate.receive[token] = None;
            fstate.free.push(token).unwrap();
            //println!("Token release {} {} µs", token, (Instant::now() - delay).as_micros());
            // BUG: freeing the token on future cancelation does not cancel the packet reception, so the received packet may interfere with any next PDU using this token
        });

        //let mut notification = self.received.notified();
        self.sendable.notify_one();
        self.received.notified().await;

        // waiting for the answer
        loop {
            //notification.await;
            //notification = self.received.notified();
            //println!("Wait {} {} µs", token, (Instant::now() - delay).as_micros());
            if *is_ready { break; }
            self.received.notified().await;
            // if (self.pdu_state.lock().await.receive[token].as_ref().unwrap().ready) { break; }
        };

        let state = self.pdu_state.lock().await;

        state.receive[token].as_ref().unwrap().answers
    }

    /// Extract a received frame of PDUs and buffer each for reception by an eventual `self.pdu()` future waiting for it.
    async fn pdu_receive(&self, frame: &[u8]) -> EthercatResult<()> {
        let mut state: LockGuard<'_, PduState> = self.pdu_state.lock().await;
        let mut frame: Cursor<&[u8]> = Cursor::new(frame);
        let _finisher = Finally::new(|| self.received.notify_waiters());
        loop {
            let header = frame.unpack::<PduHeader>()
                .map_err(|_| EthercatError::Protocol("unable to unpack PDU header, skiping all remaning PDUs in frame"))?;
            let token = usize::from(header.token());

            if token >= state.receive.len() {
                return Err(EthercatError::Protocol("received inconsistent PDU token")) }

            if let Some(storage) = state.receive[token].as_mut() {
                let content = frame.read(usize::from(u16::from(header.len())))
                    .map_err(|_|  EthercatError::Protocol("unable to unpack PDU size"))?;

                // Copy the PDU content in the reception buffer
                // Concurrency safety: this slice is written only by receiver task and read only once the receiver has set it ready
                storage.data.copy_from_slice(content);
                let footer = frame.unpack::<PduFooter>()
                    .map_err(|_| EthercatError::Protocol("unable to unpack PDU footer"))?;
                storage.answers = footer.working_count();
                storage.ready = true;
            }

            if !header.next() { break }
            if frame.remain().len() == 0 {
                return Err(EthercatError::Protocol("inconsistent ethercat frame size: remaining unused data after PDUs")) }
        }
        Ok(())
    }

    /**
        trigger sending the buffered PDUs, they will be sent as soon as possible by [Self::send] instead of waiting for the frame to be full or for the timeout

        Note: this method is helpful to manage the stream and make space in the buffer before sending PDUs, but does not help to make sure a PDU is sent deterministic time. To trigger the sending of a PDU, use argument `flush` of [Self::pdu]
    */
    pub async fn flush(&self) {
        let mut state = self.pdu_state.lock().await;
        if state.last_end != 0 {
            state.ready = true;
            self.sendable.notify_one();
        }
    }

    /**
        this is the socket reception handler

        it receives and process one datagram, it may be called in loop with no particular timer since the sockets are assumed blocking

        In case something goes wrong during PDUs unpacking, the PDUs successfully processed will be reported to their pending futures, the futures for PDU which caused the read to fail and all following PDUs in the frame will be left pending (since the data is corrupted, there is no way to determine which future should be aborted)
    */
    async fn task_receive(&self) -> EthercatResult {
        let mut receive = [0; MAX_ETHERCAT_FRAME];
        loop {
            let size: usize = poll_fn(|cx| self.socket.poll_receive(cx, &mut receive)).await?;
            let mut frame: Cursor<&[u8]> = Cursor::new(&receive[.. size]);

            let header = frame.unpack::<EthercatHeader>()?;
            if frame.remain().len() < header.len().value() as usize {
                return Err(EthercatError::Protocol("received frame header has inconsistent length")) }
            let content: &[u8] = &frame.remain()[.. header.len().value() as usize];

            assert!(header.len().value() as usize <= content.len());
            match header.ty() {
                EthercatType::PDU => self.pdu_receive(content).await?,
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
            let is_ready: bool = loop {
                self.sendable.notified().await;
                let state: LockGuard<'_, PduState> = self.pdu_state.lock().await;
                if state.last_end != 0  { break state.ready }
            };

            //Mutex scope
           let send: &mut [u8] = {
                let mut state: LockGuard<'_, PduState> =
                    if is_ready {
                        self.pdu_state.lock().await
                    }
                    else {
                        delay.reset(Instant::now() + self.pdu_merge_time);
                        // wait for more data until a timeout once data is present
                        (
                            // timeout for sending the batch
                            async {
                                delay.await.unwrap();
                                // TODO: handle the possible ioerror in the delay
                                let mut state = self.pdu_state.lock().await;
                                state.ready = true;
                                state
                            },
                            // wait for more data in the batch
                            async {
                                loop {
                                    self.sendable.notified().await;
                                    let state = self.pdu_state.lock().await;
                                    if state.ready { break state }
                                }
                            },
                        ).race().await
                    };

                // check header
                EthercatHeader::new(
                    u11::new((state.last_end - EthercatHeader::packed_size()) as u16),
                    EthercatType::PDU,
                    ).pack(&mut state.send).unwrap();

                //let end: usize = state.last_end;
                //let send: &mut [u8] = state.send[..end].as_mut();
               unsafe { std::slice::from_raw_parts_mut(state.send.as_mut_ptr(), state.last_end)}
            };

            // send
            // we are blocking the async machine until the send ends
            // we assume it is not a long operation to copy those data into the system buffer
            poll_fn(|cx| self.socket.poll_send(cx, &send)).await?;

            // reset state
            {
                let mut state: LockGuard<'_, PduState> = self.pdu_state.lock().await;
                state.ready = false;
                state.last_end = 0;
                state.last_start = EthercatHeader::packed_size();
            }

            self.sent.notify_waiters();
        }
    }

    async fn task_timeout(&self) -> EthercatResult {
        let timeout = Duration::from_millis(100);
        let mut delay = Interval::new_interval(timeout)?;
        loop {
            //Wait next tick...and check timeout status
            delay.next().await.unwrap().unwrap();
            {
                let mut state = self.pdu_state.lock().await;
                let date: Instant = Instant::now();
                for (token, storage) in state.receive.iter_mut().enumerate() {
                    if let Some(storage) = storage.as_mut() {
                        if date.duration_since(storage.sent) > timeout {
                            storage.ready = true;
                            println!("Timeout {}", token);
                        }
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
        let task = self.task.sync_lock();
        task.as_ref().map(|handle| handle.abort());
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


// callback that will be called on object drop
struct Finally<F: FnOnce()> {
    callback: Option<F>,
}
impl<F: FnOnce()> Finally<F> {
    fn new(callback: F) -> Self {Self{callback: Some(callback)}}
}
impl<F: FnOnce()>
Drop for Finally<F>  {
    fn drop(&mut self) {
        if let Some(callback) = self.callback.take() {
            callback();
        }
    }
}
