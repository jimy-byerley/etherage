//! implementation of communication with a slave's mailbox

use crate::{
	rawmaster::{RawMaster, PduCommand},
	registers,
    data::{self, PduData, Field, Storage, Cursor},
	};
use core::ops::Range;
use bilge::prelude::*;


/// arbitrary maximum size for a mailbox buffer
/// the user may select a smaller size, especially if the mailbox is used at the same time as buffered sync managers
const MAILBOX_MAX_SIZE: usize = 1024;

/**
    implementation of communication with a slave's mailbox
    
    The mailbox is a mean for ethercat slaves to implement non-minimalistic ethercat features, and features that do not fit in registers (because they rely on imperative designs, or variable size data ...).
    
    The mailbox is an optional feature of an ethercat slave.
    
    Following ETG.1000.4 6.7.1 it is using the first 2 sync managers in handshake mode. Buffered mode is not meant for mailbox.
*/
pub struct Mailbox<'a> {
    master: &'a RawMaster,
	slave: u16,
	count: u8,
	read: Range<u16>,
	write: Range<u16>,
}

impl<'b> Mailbox<'b> {
    /// condigure the mailbox on the slave, using the given `read` and `write` memory areas as mailbox buffers
    pub async fn new(master: &'b RawMaster, slave: u16, write: Range<u16>, read: Range<u16>) -> Mailbox<'b> {
        // check that there is not previous error
        if master.fprd(slave, registers::al::response).await.one().error() {
            panic!("mailbox error before init: {:?}", master.fprd(slave, registers::al::error).await.one());
        }
        
        // configure sync manager
        futures::join!(
            async { master.fpwr(slave, registers::sync_manager::interface.mailbox_write(), {
                let mut config = registers::SyncManagerChannel::default();
                config.set_address(write.start);
                config.set_length(write.end - write.start);
                config.set_mode(registers::SyncMode::Mailbox);
                config.set_direction(registers::SyncDirection::Write);
                config.set_dls_user_event(true);
                config.set_ec_event(true);
                config.set_enable(true);
                config
            }).await.one() },
            
            async { master.fpwr(slave, registers::sync_manager::interface.mailbox_read(), {
                let mut config = registers::SyncManagerChannel::default();
                config.set_address(read.start);
                config.set_length(read.end - read.start);
                config.set_mode(registers::SyncMode::Mailbox);
                config.set_direction(registers::SyncDirection::Read);
                config.set_dls_user_event(true);
                config.set_ec_event(true);
                config.set_enable(true);
                config
            }).await.one() },
        );
        
        assert!(usize::from(read.end - read.start) < MAILBOX_MAX_SIZE);
        assert!(usize::from(write.end - write.start) < MAILBOX_MAX_SIZE);
        
        Self {
            master,
            slave,
            count: 0,
            read,
            write,
        }
    }
    pub async fn poll(&mut self) -> bool {todo!()}
    pub async fn available(&mut self) -> usize {todo!()}
    
	/** 
        read the frame currently in the mailbox, wait for it if not already present
	
        `data` is the buffer to fill with the mailbox, only the first bytes corresponding to the current buffer size on the slave will be read
    
        return the slice of data received.
    */
	pub async fn read<'a>(&mut self, ty: MailboxType, priority: u2, data: &'a mut [u8]) -> &'a [u8] {
		let mailbox_control = registers::sync_manager::interface.mailbox_read();
        let mut allocated = [0; MAILBOX_MAX_SIZE];
        
        self.count = (self.count % 6)+1;
        
		// wait for data
		let mut state = loop {
            let state = self.master.fprd(self.slave, mailbox_control).await;
            if state.answers == 0 || ! state.value.mailbox_full()  {continue}
            break state.value
        };
        // the destination data is expected to be big enough for the data, so we will read only this data size
        let range = .. allocated.len()
                        .min(data.len() + MailboxHeader::packed_size())
                        .min((self.read.end - self.read.start) as usize);
        let buffer = &mut allocated[range];
		// read the mailbox content
		loop {
            if self.master.pdu(PduCommand::FPRD, self.slave, self.read.start, buffer).await == 1 
                {break}
            
            // trigger repeat
            state.set_repeat(true);
            while self.master.fpwr(self.slave, mailbox_control, state).await.answers == 0  {}
            // wait for repeated data to be available
            loop {
                let state = self.master.fprd(self.slave, mailbox_control).await;
                if state.answers == 0 || ! state.value.repeat_ack()  {continue}
                break
            }
        }
        let mut frame = Cursor::new(buffer.as_mut());
        let mut received = Cursor::new(data);
        let header = frame.unpack::<MailboxHeader>().unwrap();
        assert!(usize::from(header.length()) <= received.remain().len());
        assert_eq!(header.ty(), ty);
        assert_eq!(u8::from(header.count()), self.count);
        received.write(frame.read(header.length() as usize).unwrap()).unwrap();
		
		received.finish()
	}
	/**
        write the given frame in the mailbox, wait for it first if already busy
    */
	pub async fn write(&mut self, ty: MailboxType, priority: u2, data: &[u8]) {
		let mailbox_control = registers::sync_manager::interface.mailbox_write();
        let mailbox_size = usize::from(self.write.end - self.write.start);
        let mut allocated = [0; MAILBOX_MAX_SIZE];
        let buffer = &mut allocated[.. mailbox_size];
        
        self.count = (self.count % 6)+1;
        
        let mut frame = Cursor::new(buffer.as_mut());
        frame.pack(&MailboxHeader::new(
				data.len() as u16,
				u16::new(0),  // address of master
				u6::new(0),  // this value has no effect and is reserved for future use
				priority,
				ty,
				u3::new(self.count),
			)).unwrap();
        frame.write(data).unwrap();
        let sent = frame.finish();
        
		// wait for mailbox to be empty
		loop {
            let state = self.master.fprd(self.slave, mailbox_control).await;
            if state.answers == 0 || state.value.mailbox_full()  {continue}
            break
        }
        // write data
        // we are forced to write the whole buffer (even if much bigger than data) because the slave will notice the data sent only if writing the complete buffer
        // and writing the last byte instead does not work trick it.
//         if mailbox_size - data.len() > 32 {
//             loop {
//                 // write beginning of buffer and last byte for slave triggering
//                 let (writing, end) = futures::join!(
//                     self.master.pdu(PduCommand::FPWR, self.slave, self.write.start, sent),
//                     // the mailbox processing is done once the last mailbox byte is written, so write the last byte alone
//                     self.master.fpwr(self.slave, Field::<u8>::simple(usize::from(self.write.end)-1), 0),
//                     );
//                 if end.answers == 1 && writing == 1
//                     {break}
//             }
//         }
//         else {
            // write the full buffer
            while self.master.pdu(PduCommand::FPWR, self.slave, self.write.start, buffer.as_mut()).await != 1
                {}
//         }
	}
}

/// ETG 1000.4 table 29
#[bitsize(48)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct MailboxHeader {
    /// length of the mailbox service data following this header
    length: u16,
    /**
        - if a master is client: Station Address of the source
        - if a slave is client: Station Address of the destination
    */
    address: u16,
    /// reserved for future
    channel: u6,
    /// 0 is lowest priority, 3 is highest
    priority: u2,
    ty: MailboxType,
    /// Counter of the mailbox services (0 reserved, this should roll from 1 to 7 and overflow to 1 after 7)
    count: u3,
    reserved: u1,
}
data::bilge_pdudata!(MailboxHeader, u48);

/// ETG 1000.4 table 29
#[bitsize(4)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum MailboxType {
    Exception = 0x0,
    Ads = 0x1,
    Ethernet = 0x2,
    Can = 0x3,
    File = 0x4,
    Servo = 0x5,
    Specific = 0xf,
}

/// ETG 1000.4 table 30
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
struct MailboxErrorFrame {
    ty: u16,
    detail: MailboxError,
}
data::bilge_pdudata!(MailboxErrorFrame, u32);

// ETG 1000.4 table 30
#[bitsize(16)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum MailboxError {
    Syntax = 0x1,
    UnsupportedProtocol = 0x2,
    InvalidChannel = 0x3,
    ServiceNotSupported = 0x4,
    InvalidHeader = 0x5,
    SizeTooShort = 0x6,
    NoMoreMemory = 0x7,
    InvalidSize = 0x8,
    ServiceInWork = 0x9,
}


