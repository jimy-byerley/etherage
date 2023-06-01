use crate::rawmaster::RawMaster;
use crate::data::Field;
use bilge::prelude::*;


/**
    implementation of communication with a slave's mailbox
*/
pub struct Mailbox<'a> {
    master: &'a dyn RawMaster,
	slave: u16,
	count: u8,
}
impl Mailbox<'_> {
    /// read the frame currently in the mailbox, wait for it if not already present
    /// `data` is the buffer to fill with the mailbox, only the first bytes corresponding to the current buffer size on the slave will be read
    /// this function does not return the data size read, so the read frame should provide a length
	pub async fn read(&mut self, ty: MailboxType, priority: u2, data: &mut [u8]) {
		let mailbox = registers::sync_managers.mailbox_out();
		// inform the mailbox we want to read it
		while ! master.fprd(self.slave, mailbox.read, true).await.worked() {}
		let received = loop {
			// read data
			if let Worked(data) = master.fprd(self.slave, mailbox.address) {
				break data;
			}
			// transmission loss, wait for repetition
			else {
				while ! master.fpwr(self.slave, mailbox.repeat, true).await.worked()  {}
				while ! master.fprd(self.slave, mailbox.ready).await.unwrap_or(false) {}
			}
		};
		data.write_from_slice(received);
	}
	/// write the given frame in the mailbox
	pub async fn write(&mut self, ty: MailboxType, priority: u2, data: &[u8]) {
        self.count = (count % 6)+1;
		let mailbox = registers::sync_managers.mailbox_in();
		let sent = MailboxFrame::new(
			MailboxHeader {
				length: data.len(),
				address: self.slave,
				channel: 0,  // this value has no effect and is reserved for future use
				priority,
				ty,
				count: self.count,
			},
			data,
			);
		while ! master.fprd(self.slave, mailbox.write, true).await.worked() {}
		let received = loop {
			// read data
			if let Worked(_) = master.fpwr(self.slave, mailbox.address, sent) {
				break data;
			}
		};
	}
}

/// ETG 1000.4 5.6
struct MailboxFrame<'a> {
    header: MailboxHeader,
    data: &'a [u8],
}

/// ETG 1000.4 table 29
#[bitsize(48)]
#[derive(TryFromBits, DebugBits)]
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

/// ETG 1000.4 table 29
#[bitsize(4)]
#[derive(TryFromBits, Debug)]
enum MailboxType {
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
struct MailboxErrorFrame {
    ty: u16,
    detail: MailboxError,
}

// ETG 1000.4 table 30
#[bitsize(16)]
#[derive(TryFromBits, Debug)]
enum MailboxError {
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


const mailbox_buffers: [Field<MailboxFrame>; 3] = [
	Field::new(0x1000, 0x100),
	Field::new(0x1100, 0x100),
	Field::new(0x1200, 0x100),
	];

