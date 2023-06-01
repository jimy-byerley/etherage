use crate::mailbox::{Mailbox, MailboxType};
use bilge::prelude::*;



/// maximum byte size of sdo data that can be expedited
const EXPEDITED_MAX_SIZE: usize = 4;
/// maximum byte size of an sdo data that can be put in a sdo segment
/// it is constrained by the mailbox buffer size on the slave
const SEGMENT_MAX_SIZE: usize = 0x100 - CanHeader::ByteArray::len() 
                                      - SdoHeader::ByteArray::len();


/**
    implementation of CoE (Canopen Over Ethercat)
    
    It works exactly as in a Can bus, except each of its frame is encapsulated in a mailbox frame
*/
pub struct Can<'a> {
    mailbox: Mailbox<'a>,
}
impl Can<'_> {
    /// read and SDO, any size
	pub async fn sdo_read<T>(&mut self, address: Sdo<T>, priority: u2) -> T   {
		let mut data = T::ByteArray::new(0);
        let mut response_buf = [0; SEGMENT_MAX_SIZE 
                                    + CanHeader::ByteArray::len() 
                                    + SdoHeader::ByteArray::len()];
		let coe = CanHeader {
			number: 0,
			service: CanService::SDORequest,
			};
        // generic request
        self.mailbox.write(MailboxType::Can, address, priority, &CoeFrame::new(
            coe,
            &SdoFrame::new(
                SdoHeader::new(
                    false,
                    false,
                    false,
                    sdo.sub.is_complete(),
                    SdoCommand::Upload,
                    sdo.index,
                    sdo.sub.unwrap(),
                ),
                &[],
            ).pack(),
        ).pack());
        // receive data
        self.mailbox.read(MailboxType::Can, address, priority, &mut response_buf);
        let response = SdoFrame::unpack(
                        CoeFrame::unpack(response_buf).unwrap()
                            .data
                        ).unwrap();
        assert_eq!(response.command(), SdoCommand::UploadResponse);
        assert!(response.sized());
        assert_eq!(response.index(), sdo.index);
        assert_eq!(response.sub(), sdo.sub.unwrap());
        
        if response.expedited() {
            // expedited transfer
            T::unpack(&response.data[.. EXPEDITED_MAX_SIZE - usize::from(response.size())])
        }
        else {
            // normal transfer
            assert_eq!(response.size, data.len());
            
            let mut received = 0;
            let mut toggle = true;
            data[received ..][.. response.data.len()].copy_from_slice(received.data);
            received += received.data.len();
            
            // receive more data from segments
            while received < data.len() {
                assert!(response.more());
                
                self.mailbox.read(MailboxType::Can, address, priority, &mut response_buf);
                let response = SegmentFrame::unpack(
                                CoeFrame::unpack(response_buf).unwrap()
                                    .data
                                ).unwrap();
                assert_eq!(response.toggle, toggle);
                data[received ..][.. response.data.len()].copy_from_slice(received.data);
                received += received.data.len();
                toggle = ! toggle;
            }
            assert!(response.more());
            assert_eq!(received, data.len());
            
            T::unpack(&data)
        }
	}
	/// write SDO, any size
	pub async fn sdo_write<T>(&mut self, sdo: Sdo<T>, priority: u2, data: T)  {
		let data = data.pack();
		
        let mut response_buf = [0; 12];
		let coe = CanHeader {
			number: 0,
			service: CanService::SDORequest,
			};
		if data.len() < EXPEDITED_MAX_SIZE.len() {
			// expedited transfer
			// send data in the 4 bytes instead of data size
			self.mailbox.write(MailboxType::Can, priority, &CoeFrame::new(
				coe,
				&SdoFrame::new(
					SdoHeader {
                        sized: true,
                        expedited: true,
                        size: EXPEDITED_MAX_SIZE - data.len(),
                        complete: sdo.sub.is_complete(),
                        command: SdoCommand::DownloadRequest,
                        index: sdo.index,
                        sub: sdo.sub.unwrap(),
                    },
					data,
				).pack(),
            ).pack());
            
            // receive acknowledge
            self.mailbox.read(MailboxType::Can, priority, &mut response_buf);
            let response = SdoFrame::unpack(
                        CoeFrame::unpack(buf).unwrap()
                            .data
                        ).unwrap()
                        .header;
            assert_eq!(response.command, SdoCommand::DownloadResponse);
            assert_eq!(response.index, sdo.index);
            assert_eq!(response.sub, sdo.sub.unwrap());
		}
		else {
			// normal transfer
			let mut sent = 0;
			
			// send one download request with the start of data
			let segment = data.len().min(MAILBOX_MAX_SIZE - SdoHeader::ByteArray::len());
			self.mailbox.write(MailboxType::Can, priority, CoeFrame::new(
				coe,
				&SdoFrame::new(
					SdoHeader {
                        sized: true,
                        expedited: false,
                        size: 0,
                        complete: sdo.sub.is_complete(),
                        command: SdoCommand::DownloadRequest,
                        index: sdo.index,
                        sub: sdo.sub.unwrap(),
                    },
					Some(segment),
					data[sent..][..segment],
				),
            ));
            // receive acknowledge
            self.mailbox.read(MailboxType::Can, priority, response_buf);
            let response = SdoFrame::unpack(
                        CoeFrame::unpack(response_buf).unwrap()
                            .data
                        ).unwrap()
                        .header;
            assert_eq!(response.command, SdoCommand::DownloadResponse);
            assert_eq!(response.index, sdo.index);
            assert_eq!(response.sub, sdo.sub.unwrap());
            sent += segment;
            
            // send many segments for the rest of the data, aknowledge each time
            let mut toggle = false;
            while sent < data.len() {
                // send segment
                let max_segment = MAILBOX_MAX_SIZE - SdoSegment::ByteArray::len();
                let (segment, more) = if data.len() < max_segment
                    {(data.len(), false)} else {(max_segment, true)};
                self.mailbox.write(MailboxType::Can, priority, CoeFrame::new(
                    coe,
                    SdoFrame::new(
                        SdoSegment {
                            more,
                            size: 0,
                            toggle,
                        },
                        data[sent..][..segment],
                    ),
                ));
                // aknowledge
                self.mailbox.read(MailboxType::Can, priority, response_buf);
                let response = SegmentFrame::unpack(
                        CoeFrame::unpack(response_buf).unwrap()
                            .data
                        ).unwrap()
                        .header;
                assert_eq!(response.command, SdoCommand::DownloadSegmentResponse);
                assert_eq!(response.toggle, toggle);
                toggle = !toggle;
                sent += segment;
            }
		}
	}
	
	pub fn pdo_read() {todo!()}
	pub fn pdo_write() {todo!()}
	
	pub fn info_dictionnary() {todo!()}
	pub fn info_sdo() {todo!()}
	pub fn info_subitem() {todo!()}
}

struct SdoFrame<'a> {
    header: SdoHeader,
    data: &'a [u8],
}

struct SegmentFrame<'a> {
    header: SegmentHeader,
    data: &'a [u8],
}

#[bitsize(16)]
struct CanHeader {
    /// present in the Can protocol, but not used in CoE
    number: u9,
    reserved: u3,
    /// Can command
    service: CanService,
}

/**
    Type of can service
    
    receiving and transmiting is from the point of view of the slave: 
        - transmitting is slave -> master
        - receiving is master -> slave
*/
#[bitsize(4)]
enum CanService {
    Emergency = 0x1,
    SdoRequest = 0x2,
    SdoResponse = 0x3,
    TransmitPdo = 0x4,
    ReceivePdo = 0x5,
    TransmitPdoRemoteRequest = 0x6,
    ReceivePdoRemoteRequest = 0x7,
    SdoInformation = 0x8,
}

/// Header for operations with SDOs
///
/// ETG.1000.6 5.6.2
#[bitsize(32)]
struct SdoHeader {
    /// true if field `size` is used
    sized: bool,
    /// true in case of an expedited transfer (the data size specified by `size`)
    expedited: bool,
    /// indicate the data size but not as an integer.
    /// this value shall be `SDO_REQUESTS_SIZES[sizeof]`
    size: u2,
    /// true if a complete SDO is accessed
    complete: bool,
    /// operation to perform with the indexed SDO
    command: SdoCommand,
    /// SDO index
    index: u16,
    /**
    - if subitem is accessed: SDO subindex
    - if complete item is accessed:
        + put 0 to include subindex 0 in transmission
        + put 1 to exclude subindex 0 from transmission
    */
    sub: u8,
}
#[bitsize(16)]
struct SdoSegment {
    more: bool,
    size: u3,
    toggle: bool,
    command: SdoCommand,
}

/// operation to perform with an SDO in CoE
///
/// ETG.1000.6 5.6.2.1-7
#[bitsize(3)]
enum SdoCommand {
    DownloadRequest = 0x1,
    DownloadResponse = 0x3,
    DownloadSegmentRequest = 0x0,
    DownloadSegmentResponse = 0x1,
    UploadRequest = 0x2,
    UploadResponse = 0x2,
    UploadSegmentRequest = 0x3,
    UploadSegmentResponse = 0x0,
    Abort = 0x4,
}

impl<'a> SdoFrame<'a> {
    fn pack(&self, dst: &mut [u8]) {
        let cursor = Cursor::new(dst);
        dst.write(CanHeader {
            number: 0,
            service: CanService::SdoRequest,
            });
        dst.write(self.header.pack()).unwrap();
        dst.write(self.data).unwrap();
    }
}

enum SdoAbortCode {
    /// Toggle bit not changed
    0x05_03_00_00,
    /// SDO protocol timeout
    0x05_04_00_00,
    /// Client/Server command specifier not valid or unknown
    0x05_04_00_01,
    /// Out of memory
    0x05_04_00_05,
    /// Unsupported access to an object
    0x06_01_00_00,
    /// Attempt to read to a write only object
    0x06_01_00_01,
    /// Attempt to write to a read only object
    0x06_01_00_02,
    /// Subindex cannot be written, SI0 must be 0 for write access
    0x06_01_00_03,
    /// SDO Complete access not supported for objects of variable length such as ENUMobject types

    0x06_01_00_04,
    /// Object length exceeds mailbox size
    0x06_01_00_05,
    /// Object mapped to RxPDO, SDO Download blocked
    0x06_01_00_06,
    /// The object does not exist in the object directory
    0x06_02_00_00,
    /// The object can not be mapped into the PDO
    0x06_04_00_41,
    /// The number and length of the objects to be mapped would exceed the PDO length
    0x06_04_00_42,
    /// General parameter incompatibility reason
    0x06_04_00_43,
    /// General internal incompatibility in the device
    0x06_04_00_47,
    /// Access failed due to a hardware error
    0x06_06_00_00,
    /// Data type does not match, length of service parameter does not match
    0x06_07_00_10,
    /// Data type does not match, length of service parameter too high
    0x06_07_00_12,
    /// Data type does not match, length of service parameter too low
    0x06_07_00_13,
    /// Subindex does not exist
    0x06_09_00_11,
    /// Value range of parameter exceeded (only for write access)
    0x06_09_00_30,
    /// Value of parameter written too high
    0x06_09_00_31,
    /// Value of parameter written too low
    0x06_09_00_32,
    /// Maximum value is less than minimum value
    0x06_09_00_36,
    /// General error
    0x08_00_00_00,
    /// Data cannot be transferred or stored to the applicationNOTE: This is the general Abort Code in case no further detail on the reason can determined. It is recommended to use one of the more detailed Abort Codes (0x08000021, 0x08000022)

    0x08_00_00_20,
    /** Data cannot be transferred or stored to the application because of local control
    NOTE: “local control” means an application specific reason. It does not mean the
    ESM-specific control
    */
    0x08_00_00_21,
    /** Data cannot be transferred or stored to the application because of the present
    device state
    NOTE: “device state” means the ESM state
    */
    0x08_00_00_22,
    /// Object dictionary dynamic generation fails or no object dictionary is present
    0x08_00_00_23,
}
