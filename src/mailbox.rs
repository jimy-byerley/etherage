use crate::{
	socket::EthercatSocket,
	rawmaster::{RawMaster, PduCommand},
	registers,
    data::{self, PduData, Storage, PackingResult, Cursor},
	};
use bilge::prelude::*;


/**
    implementation of communication with a slave's mailbox
    
    Following ETG.1000.4 6.7.1 it is using the first 2 sync managers in handshake mode. Buffered mode is not meant for mailbox.
*/
pub struct Mailbox<'a> {
    master: &'a RawMaster,
	slave: u16,
	count: u8,
}

impl<'b> Mailbox<'b> {	
    pub fn new(master: &'b RawMaster, slave: u16) -> Self {
        Self {
            master,
            slave,
            count: 0,
        }
    }
    pub async fn poll(&mut self) -> bool {todo!()}
    pub async fn available(&mut self) -> usize {todo!()}
    
	/// read the frame currently in the mailbox, wait for it if not already present
    /// `data` is the buffer to fill with the mailbox, only the first bytes corresponding to the current buffer size on the slave will be read
    /// this function does not return the data size read, so the read frame should provide a length
	pub async fn read<'a>(&mut self, ty: MailboxType, priority: u2, data: &'a mut [u8]) -> &'a [u8] {
		let mailbox_control = registers::sync_manager::interface.mailbox_read();
		let mailbox_buffer = &registers::mailbox_buffers[0];
        let mut allocated = [0; registers::mailbox_buffers[0].len];
        
		// wait for data
		let mut state = loop {
            let state = self.master.fprd(self.slave, mailbox_control).await;
            if state.answers == 0 || ! state.value.mailbox_full()  {continue}
            break state.value
        };
        // the destination data is expected to be big enough for the data, so we will read only this data size
        let range = .. allocated.len().min(data.len() + MailboxHeader::packed_size());
        let buffer = &mut allocated[range];
		// read the mailbox content
		loop {
            if self.master.pdu(PduCommand::FPRD, self.slave, mailbox_buffer.byte as u16, buffer).await == 1 
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
		
		// TODO wait for mailbox to become ready again ?
		
		received.finish()
	}
	/// write the given frame in the mailbox
	pub async fn write(&mut self, ty: MailboxType, priority: u2, data: &[u8]) {
		let mailbox_control = registers::sync_manager::interface.mailbox_write();
		let mailbox_buffer = &registers::mailbox_buffers[0];
        let mut buffer = [0; registers::mailbox_buffers[0].len];
		
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
		let state = loop {
            let state = self.master.fprd(self.slave, mailbox_control).await;
            if state.answers == 0 || state.value.mailbox_full()  {continue}
            break state.value
        };
        // write data
        while ! self.master.pdu(PduCommand::FPWR, self.slave, mailbox_buffer.byte as u16, sent).await == 1 
            {}
		
		// TODO wait for mailbox to become ready again ?
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


