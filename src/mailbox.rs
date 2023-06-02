use crate::{
	socket::EthercatSocket,
	rawmaster::{RawMaster, PduCommand},
	registers,
    data::{self, ByteArray},
	};
use bilge::prelude::*;


/**
    implementation of communication with a slave's mailbox
*/
pub struct Mailbox<'a> {
    master: &'a RawMaster<impl EthercatSocket>,
	slave: u16,
	count: u8,
}

impl Mailbox<'_> {	
    pub async fn poll(&mut self) -> bool {todo!()}
    
	/// read the frame currently in the mailbox, wait for it if not already present
    /// `data` is the buffer to fill with the mailbox, only the first bytes corresponding to the current buffer size on the slave will be read
    /// this function does not return the data size read, so the read frame should provide a length
	pub async fn read(&mut self, ty: MailboxType, priority: u2, data: &mut [u8]) {
		let mailbox = registers::sync_manager::interface.mailbox_out();
		// wait for data
		let state = loop {
            let state = self.master.fprd(self.slave, mailbox).await;
            if state.answers == 0 || ! state.value.mailbox_full()  {continue}
            break state.value
        };
        let mut allocated = [0; registers::mailbox_buffers[0].size];
        // the destination data is expected to be big enough for the data, so we will read only this data size
        let buffer = &mut allocated[.. allocated.len()
                                    .min(data.len() + MailboxHeader::packed_size())];
		// read the mailbox content
		loop {
            let reading = self.master.pdu(PduCommand::FPRD, self.slave, address, buffer).await;
            if reading.answers == 1 {break}
            
            // trigger repeat
            state.set_repeat(true);
            while self.master.fpwr(self.slave, mailbox, state).await.answers == 0  {}
            // wait for repeated data to be available
            loop {
                let state = self.master.fprd(self.slave, mailbox).await;
                if state.answers == 0 || ! state.value.repeat_ack()  {continue}
                break
            }
        }
        let frame = MailboxFrame::unpack(buffer).unwrap();
        assert!(frame.header.length() <= data.len());
        assert_eq!(frame.header.ty, ty);
        assert_eq!(frame.header.count, self.count);
		data[.. frame.data.len()].write_from_slice(frame.data);
		
		// TODO wait for mailbox to become ready again ?
	}
	/// write the given frame in the mailbox
	pub async fn write(&mut self, ty: MailboxType, priority: u2, data: &[u8]) {
        self.count = (self.count % 6)+1;
		let mailbox = registers::sync_manager::interface.mailbox_in();
		let frame = MailboxFrame {
			header: MailboxHeader::new(
				data.len(),
				0,  // address of master
				0,  // this value has no effect and is reserved for future use
				priority,
				ty,
				self.count,
			),
			data,
        };
        let mut buffer = [0; registers::mailbox_buffers[0].size];
        // the destination data is expected to be big enough for the data, so we will read only this data size
        assert!(allocated.len() > frame.packed_size());
        frame.pack(&mut buffer);
		// wait for mailbox to be empty
		let state = loop {
            let state = self.master.fprd(self.slave, mailbox).await;
            if state.answers == 0 || state.value.mailbox_full()  {continue}
            break state.value
        };
        // write data
        while ! self.master.pdu(PduCommand::FPWR, self.slave, mailbox, buffer[.. frame.packed_size()]).await.answers == 1 {}
		
		// TODO wait for mailbox to become ready again ?
	}
}


/// ETG 1000.4 5.6
struct MailboxFrame<'a> {
    header: MailboxHeader,
    data: &'a [u8],
}
impl<'a> MailboxFrame<'a> {
    fn packed_size(&self) -> usize {
        MailboxHeader::packed_size() 
        + self.header.length() as usize
    }
    fn pack(&self, dst: &mut [u8]) {
        let cursor = Cursor::new(dst);
        cursor.write(self.header.pack()).unwrap();
        cursor.write(self.data).unwrap();
    }
    fn unpack(src: &[u8]) -> PackingResult<Self> {
        Self {
            header: MailboxHeader::unpack(src),
            data: src[MailboxHeader::packed_size() ..][.. header.length()],
        }
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
data::bilge_pdudata!(MailboxHeader);

/// ETG 1000.4 table 29
#[bitsize(4)]
#[derive(TryFromBits, Debug, Copy, Clone)]
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
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct MailboxErrorFrame {
    ty: u16,
    detail: MailboxError,
}
data::bilge_pdudata!(MailboxErrorFrame);

// ETG 1000.4 table 30
#[bitsize(16)]
#[derive(TryFromBits, Debug, Copy, Clone)]
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


