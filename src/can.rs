use crate::{
	mailbox::{Mailbox, MailboxType},
	registers,
	sdo::Sdo,
	data::{self, PduData, ByteArray, PackingResult},
	};
use bilge::prelude::*;
use std::io::{Cursor, Write};



const MAILBOX_MAX_SIZE: usize = registers::mailbox_buffers[0].len;
/// maximum byte size of sdo data that can be expedited
const EXPEDITED_MAX_SIZE: usize = 4;
/// maximum byte size of an sdo data that can be put in a sdo request
const SDO_REQUEST_MAX_SIZE: usize = registers::mailbox_buffers[0].len 
                                        - CoeHeader::packed_size() 
                                        - SdoHeader::packed_size();
/// maximum byte size of an sdo data that can be put in a sdo segment
/// it is constrained by the mailbox buffer size on the slave
const SDO_SEGMENT_MAX_SIZE: usize = registers::mailbox_buffers[0].len
                                        - CoeHeader::packed_size() 
                                        - SdoSegmentHeader::packed_size();


/**
    implementation of CoE (Canopen Over Ethercat)
    
    It works exactly as in a Can bus, except each of its frame is encapsulated in a mailbox frame
*/
pub struct Can<'a> {
    mailbox: Mailbox<'a>,
}
impl Can<'_> {
    /// read and SDO, any size
	pub async fn sdo_read<T: PduData>(&mut self, sdo: Sdo<T>, priority: u2) -> T   {
		let mut data = T::ByteArray::new(0);
        let mut buffer = [0; MAILBOX_MAX_SIZE];
		let coe = CoeHeader {
			number: 0,
			service: CanService::SDORequest,
			};
        // generic request
        coe.pack(&mut buffer);
        buffer[CoeHeader::packed_size() ..].copy_from_slice(SdoHeader::new(
                false,
                false,
                false,
                sdo.sub.is_complete(),
                SdoCommand::Upload,
                sdo.index,
                sdo.sub.unwrap(),
            ).pack());
        self.mailbox.write(MailboxType::Can, priority, &buffer).await;
        
        // receive data
        // TODO check for possible SDO error
        let response = self.mailbox.read(MailboxType::Can, priority, &mut buffer).await;
        assert_eq!(CoeHeader::unpack(&buffer).service, CanService::SdoRequest);
        let header = SdoHeader::unpack(&buffer[CoeHeader::packed_size() ..]).unwrap();
        assert_eq!(header.command(), SdoCommand::UploadResponse);
        assert!(header.sized());
        assert_eq!(header.index(), sdo.index);
        assert_eq!(header.sub(), sdo.sub.unwrap());
        
        if header.expedited() {
            // expedited transfer
            T::unpack(&buffer
                [CoeHeader::packed_size() + SdoHeader::packed_size() ..]
                [.. EXPEDITED_MAX_SIZE - usize::from(header.size())]
                ).unwrap()
        }
        else {
            // normal transfer
            let total = u32::unpack(&buffer
                [CoeHeader::packed_size() + SdoHeader::packed_size() ..]
                ).unwrap();
            assert_eq!(total, data.len());
            
            let mut received = 0;
            let mut toggle = true;
            data[received ..][.. response.data.len()].copy_from_slice(&buffer
                [CoeHeader::packed_size() + SdoHeader::packed_size() + u32::packed_size() ..]
                );
            received += received.data.len();
            drop(response);
            
            // receive more data from segments
            // TODO check for possible SDO error
            loop {
                let response = self.mailbox.read(MailboxType::Can, priority, &mut buffer);
                assert_eq!(CoeHeader::unpack(&buffer).service, CanService::SdoRequest);
                let header = SdoSegmentHeader::unpack(&buffer
                                [CoeHeader::packed_size() ..]
                                ).unwrap();
                assert_eq!(header.toggle(), toggle);
                let content = &buffer[(total - received)
                                .min(CoeHeader::packed_size() + SdoSegmentHeader::packed_size()) ..];
                data[received ..][.. content.len()].copy_from_slice(content);
                received += content.len();
                toggle = ! toggle;
                
                assert!(received <= data.len());
                if ! response.more  {break}
            }
            assert_eq!(received, data.len());
            
            T::unpack(&data).unwrap()
        }
        
        // TODO: error propagation instead of asserts
        // TODO send SdoCommand::Abort in case any error
	}
	/// write SDO, any size
	pub async fn sdo_write<T: PduData>(&mut self, sdo: Sdo<T>, priority: u2, data: T)  {
		let packed = data.pack();
		let data = packed.as_bytes_slice();
		
        let mut buffer = [0; MAILBOX_MAX_SIZE];
		let coe = CoeHeader::new(
			u9::new(0),
			CanService::SDORequest,
			);
		if data.len() < EXPEDITED_MAX_SIZE {
			// expedited transfer
			// send data in the 4 bytes instead of data size
			buffer[.. CoeHeader::packed_size()].copy_from_slice(coe.pack().as_bytes_slice());
			buffer[CoeHeader::packed_size() ..][.. SdoHeader::packed_size()].copy_from_slice(SdoHeader::new(
                        true,
                        true,
                        u2::from(EXPEDITED_MAX_SIZE - data.len()),
                        sdo.sub.is_complete(),
                        SdoCommand::DownloadRequest,
                        sdo.index,
                        sdo.sub.unwrap(),
                    ).pack().as_bytes_slice());
            buffer[CoeHeader::packed_size() + SdoHeader::packed_size() ..][.. data.len()].copy_from_slice(data);
			self.mailbox.write(MailboxType::Can, priority, &buffer[.. CoeHeader::packed_size() + SdoHeader::packed_size() + data.len()]);
            
            // receive acknowledge
            // TODO check for possible SDO error
            self.mailbox.read(MailboxType::Can, priority, &mut buffer);
            let response = SdoFrame::unpack(
                        CoeFrame::unpack(&buffer).unwrap()
                            .data
                        ).unwrap()
                        .header;
            assert_eq!(response.command(), SdoCommand::DownloadResponse);
            assert_eq!(response.index(), sdo.index);
            assert_eq!(response.sub(), sdo.sub.unwrap());
		}
		else {
			// normal transfer
			let mut sent = 0;
			
			// send one download request with the start of data
			let segment = data.len().min(SDO_SEGMENT_MAX_SIZE);
			self.mailbox.write(MailboxType::Can, priority, CoeFrame {
				header: coe,
				data: &SdoFrame {
					header: SdoHeader::new(
                        true,
                        false,
                        u2::new(0),
                        sdo.sub.is_complete(),
                        SdoCommand::DownloadRequest,
                        sdo.index,
                        sdo.sub.unwrap(),
                    ),
					data: data[sent..][..segment],
				}.pack(),
            });
            // receive acknowledge
            // TODO check for possible SDO error
            self.mailbox.read(MailboxType::Can, priority, &mut buffer);
            let response = SdoFrame::unpack(
                        CoeFrame::unpack(&buffer).unwrap().data
                        ).unwrap().header;
            assert_eq!(response.command(), SdoCommand::DownloadResponse);
            assert_eq!(response.index(), sdo.index);
            assert_eq!(response.sub(), sdo.sub.unwrap());
            sent += segment;
            
            // send many segments for the rest of the data, aknowledge each time
            let mut toggle = false;
            while sent < data.len() {
                // send segment
                let (segment, more) = if data.len() < SDO_SEGMENT_MAX_SIZE
                    {(data.len(), false)} else {(SDO_SEGMENT_MAX_SIZE, true)};
                self.mailbox.write(MailboxType::Can, priority, CoeFrame {
                    header: coe,
                    data: SdoSegmentFrame {
                        header: SdoSegmentHeader::new(more, u3::new(0), toggle, SdoCommand::DownloadRequest),
                        data: &data[sent..][..segment],
                    }.pack(),
                });
                // aknowledge
                // TODO check for possible SDO error
                self.mailbox.read(MailboxType::Can, priority, &mut buffer);
                let response = SdoSegmentFrame::unpack(
                        CoeFrame::unpack(&buffer).unwrap().data
                        ).unwrap().header;
                assert_eq!(response.command(), SdoCommand::DownloadSegmentResponse);
                assert_eq!(response.toggle(), toggle);
                toggle = !toggle;
                sent += segment;
            }
		}
		
        // TODO send SdoCommand::Abort in case any error
	}
	
	pub fn pdo_read() {todo!()}
	pub fn pdo_write() {todo!()}
	
	pub fn info_dictionnary() {todo!()}
	pub fn info_sdo() {todo!()}
	pub fn info_subitem() {todo!()}
}

struct CoeFrame<'a> {
    header: CoeHeader,
    data: &'a [u8],
}

#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct CoeHeader {
    /// present in the Can protocol, but not used in CoE
    number: u9,
    reserved: u3,
    /// Can command
    service: CanService,
}
data::bilge_pdudata!(CoeHeader);

/**
    Type of can service
    
    receiving and transmiting is from the point of view of the slave: 
        - transmitting is slave -> master
        - receiving is master -> slave
*/
#[bitsize(4)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
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
data::bilge_pdudata!(CanService);


use crate::data::FrameData;
use core::marker::PhantomData;

struct SdoFrame<'a, T: FrameData<'a>> {
    header: SdoHeader,
    data: T,
    phantom: PhantomData<&'a ()>,
}

impl<'a, T: FrameData<'a>>   FrameData<'a> for SdoFrame<'a, T> {
    fn pack(&self, dst: &mut [u8]) -> PackingResult<()> {
        dst[.. SdoHeader::packed_size()].copy_from_slice(&self.header.pack());
        self.data.pack(&mut dst[SdoHeader::packed_size() ..])?;
        Ok(())
    }
    fn unpack(src: &'a [u8]) -> PackingResult<Self> {
		let header = SdoHeader::unpack(src)?;
        Ok(Self {
            header,
            data: T::unpack(&src[SdoHeader::packed_size() ..][.. header.length() as usize])?,
        })
    }
}


/// Header for operations with SDOs
///
/// ETG.1000.6 5.6.2
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
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
data::bilge_pdudata!(SdoHeader);

struct SdoSegmentFrame<'a> {
    header: SdoSegmentHeader,
    data: &'a [u8],
}

#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct SdoSegmentHeader {
    more: bool,
    size: u3,
    toggle: bool,
    command: SdoCommand,
}
data::bilge_pdudata!(SdoSegmentHeader);

/// operation to perform with an SDO in CoE
///
/// ETG.1000.6 5.6.2.1-7
#[bitsize(3)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
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
data::bilge_pdudata!(SdoCommand);


// enum SdoAbortCode {
//     /// Toggle bit not changed
//     0x05_03_00_00,
//     /// SDO protocol timeout
//     0x05_04_00_00,
//     /// Client/Server command specifier not valid or unknown
//     0x05_04_00_01,
//     /// Out of memory
//     0x05_04_00_05,
//     /// Unsupported access to an object
//     0x06_01_00_00,
//     /// Attempt to read to a write only object
//     0x06_01_00_01,
//     /// Attempt to write to a read only object
//     0x06_01_00_02,
//     /// Subindex cannot be written, SI0 must be 0 for write access
//     0x06_01_00_03,
//     /// SDO Complete access not supported for objects of variable length such as ENUMobject types
// 
//     0x06_01_00_04,
//     /// Object length exceeds mailbox size
//     0x06_01_00_05,
//     /// Object mapped to RxPDO, SDO Download blocked
//     0x06_01_00_06,
//     /// The object does not exist in the object directory
//     0x06_02_00_00,
//     /// The object can not be mapped into the PDO
//     0x06_04_00_41,
//     /// The number and length of the objects to be mapped would exceed the PDO length
//     0x06_04_00_42,
//     /// General parameter incompatibility reason
//     0x06_04_00_43,
//     /// General internal incompatibility in the device
//     0x06_04_00_47,
//     /// Access failed due to a hardware error
//     0x06_06_00_00,
//     /// Data type does not match, length of service parameter does not match
//     0x06_07_00_10,
//     /// Data type does not match, length of service parameter too high
//     0x06_07_00_12,
//     /// Data type does not match, length of service parameter too low
//     0x06_07_00_13,
//     /// Subindex does not exist
//     0x06_09_00_11,
//     /// Value range of parameter exceeded (only for write access)
//     0x06_09_00_30,
//     /// Value of parameter written too high
//     0x06_09_00_31,
//     /// Value of parameter written too low
//     0x06_09_00_32,
//     /// Maximum value is less than minimum value
//     0x06_09_00_36,
//     /// General error
//     0x08_00_00_00,
//     /// Data cannot be transferred or stored to the applicationNOTE: This is the general Abort Code in case no further detail on the reason can determined. It is recommended to use one of the more detailed Abort Codes (0x08000021, 0x08000022)
// 
//     0x08_00_00_20,
//     /** Data cannot be transferred or stored to the application because of local control
//     NOTE: “local control” means an application specific reason. It does not mean the
//     ESM-specific control
//     */
//     0x08_00_00_21,
//     /** Data cannot be transferred or stored to the application because of the present
//     device state
//     NOTE: “device state” means the ESM state
//     */
//     0x08_00_00_22,
//     /// Object dictionary dynamic generation fails or no object dictionary is present
//     0x08_00_00_23,
// }
