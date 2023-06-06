use crate::{
	mailbox::{Mailbox, MailboxType},
	registers,
	sdo::Sdo,
	data::{self, PduData, Storage, PackingResult, Cursor},
	};
use bilge::prelude::*;



const MAILBOX_MAX_SIZE: usize = registers::mailbox_buffers[0].len;
/// maximum byte size of sdo data that can be expedited
const EXPEDITED_MAX_SIZE: usize = 4;
/// maximum byte size of an sdo data that can be put in a sdo request
const SDO_REQUEST_MAX_SIZE: usize = registers::mailbox_buffers[0].len 
                                        - <CoeHeader as PduData>::Packed::LEN 
                                        - <SdoHeader as PduData>::Packed::LEN;
/// maximum byte size of an sdo data that can be put in a sdo segment
/// it is constrained by the mailbox buffer size on the slave
const SDO_SEGMENT_MAX_SIZE: usize = registers::mailbox_buffers[0].len
                                        - <CoeHeader as PduData>::Packed::LEN
                                        - <SdoSegmentHeader as PduData>::Packed::LEN;


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
		let mut data = T::Packed::uninit();
        let mut buffer = [0; MAILBOX_MAX_SIZE];
        // generic request
        let mut frame = Cursor::new(buffer.as_mut_slice());
        frame.pack(&CoeHeader::new(u9::new(0), CanService::SdoRequest)).unwrap();
        frame.pack(&SdoHeader::new(
                false,  // uninit
                false,  // uninit
                u2::new(0),  // uninit
                sdo.sub.is_complete(),
                u3::from(SdoCommandRequest::Upload),
                sdo.index,
                sdo.sub.unwrap(),
            )).unwrap();
        self.mailbox.write(MailboxType::Can, priority, frame.finish()).await;
        
        // receive data
        // TODO check for possible SDO error
        let mut frame = Cursor::new(self.mailbox.read(MailboxType::Can, priority, &mut buffer).await);
        assert_eq!(frame.unpack::<CoeHeader>().unwrap().service(), CanService::SdoResponse);
        let header = frame.unpack::<SdoHeader>().unwrap();
        assert_eq!(SdoCommandResponse::try_from(header.command()).unwrap(), SdoCommandResponse::Upload);
        assert!(header.sized());
        assert_eq!(header.index(), sdo.index);
        assert_eq!(header.sub(), sdo.sub.unwrap());
        
        if header.expedited() {
            // expedited transfer
            T::unpack(frame.read(EXPEDITED_MAX_SIZE - u8::from(header.size()) as usize).unwrap()).unwrap()
        }
        else {
            // normal transfer, eventually segmented
            let total = frame.unpack::<u32>().unwrap().try_into().expect("SDO is too big for memory");
            assert_eq!(total, T::Packed::LEN);
            
            let mut received = Cursor::new(&mut data.as_mut()[.. total]);
            let mut toggle = true;
            received.write(frame.remain()).unwrap();
            
            // receive more data from segments
            // TODO check for possible SDO error
            loop {
				// send segment request
                {
                    let mut frame = Cursor::new(buffer.as_mut_slice());
                    frame.pack(&CoeHeader::new(u9::new(0), CanService::SdoRequest)).unwrap();
                    frame.pack(&SdoSegmentHeader::new(
                            false, 
                            u3::new(0), 
                            toggle, 
                            u3::from(SdoCommandRequest::UploadSegment),
                        )).unwrap();
                    self.mailbox.write(MailboxType::Can, priority, frame.finish()).await;
                }
            
				// receive segment
				{
					let mut frame = Cursor::new(self.mailbox.read(MailboxType::Can, priority, &mut buffer).await);
					assert_eq!(frame.unpack::<CoeHeader>().unwrap().service(), CanService::SdoResponse);
					let header = frame.unpack::<SdoSegmentHeader>().unwrap();
					assert_eq!(SdoCommandResponse::try_from(header.command()).unwrap(), SdoCommandResponse::UploadSegment);
					assert_eq!(header.toggle(), toggle);
					let segment = frame.read(received.remain().len().min(frame.remain().len())).unwrap();
					received.write(segment).unwrap();
					
					// TODO: check for abort
					
					assert!(received.position() <= T::Packed::LEN);
					if ! header.more () {break}
                }
                
				toggle = ! toggle;
            }
            assert_eq!(received.position(), T::Packed::LEN);
            
            T::unpack(data.as_ref()).unwrap()
        }
        
        // TODO: error propagation instead of asserts
        // TODO send SdoCommand::Abort in case any error
	}
	/// write SDO, any size
	pub async fn sdo_write<T: PduData>(&mut self, sdo: Sdo<T>, priority: u2, data: T)  {		
        let mut buffer = [0; MAILBOX_MAX_SIZE];
		if T::Packed::LEN < EXPEDITED_MAX_SIZE {
			// expedited transfer
			// send data in the 4 bytes instead of data size
			{
                let mut frame = Cursor::new(buffer.as_mut_slice());
                frame.pack(&CoeHeader::new(u9::new(0), CanService::SdoRequest)).unwrap();
                frame.pack(&SdoHeader::new(
                            true,
                            true,
                            u2::from((EXPEDITED_MAX_SIZE - T::Packed::LEN) as u8),
                            sdo.sub.is_complete(),
                            u3::from(SdoCommandRequest::Download),
                            sdo.index,
                            sdo.sub.unwrap(),
                        )).unwrap();
                frame.pack(&data).unwrap();
                self.mailbox.write(MailboxType::Can, priority, frame.finish()).await;
            }
            
            // receive acknowledge
//             let response = self.mailbox.read::<CoeFrame<SdoFrame<'_>>>
//                             (MailboxType::Can, priority, &mut buffer);
            {
                let mut frame = Cursor::new(self.mailbox.read(MailboxType::Can, priority, &mut buffer).await);
                let header = frame.unpack::<CoeHeader>().unwrap();
                assert_eq!(frame.unpack::<CoeHeader>().unwrap().service(), CanService::SdoResponse);
                let header = frame.unpack::<SdoHeader>().unwrap();
                assert_eq!(SdoCommandResponse::try_from(header.command()).unwrap(), SdoCommandResponse::Download);
                assert_eq!(header.index(), sdo.index);
                assert_eq!(header.sub(), sdo.sub.unwrap());
            }
		}
		else {
			// normal transfer, eventually segmented
			let mut packed = T::Packed::uninit();
			data.pack(packed.as_mut()).unwrap();
			let mut data = Cursor::new(packed.as_ref());
			
			// send one download request with the start of data
			{
                let mut frame = Cursor::new(buffer.as_mut_slice());
                frame.pack(&CoeHeader::new(u9::new(0), CanService::SdoRequest)).unwrap();
                frame.pack(&SdoHeader::new(
                            true,
                            false,
                            u2::new(0),
                            sdo.sub.is_complete(),
                            u3::from(SdoCommandRequest::Download),
                            sdo.index,
                            sdo.sub.unwrap(),
                        )).unwrap();
                let segment = data.remain().len().min(SDO_SEGMENT_MAX_SIZE);
                frame.write(data.read(segment).unwrap()).unwrap();
                self.mailbox.write(MailboxType::Can, priority, frame.finish()).await;
            }
			
            // receive acknowledge
            {
                // TODO check for possible SDO error
                let mut frame = Cursor::new(self.mailbox.read(MailboxType::Can, priority, &mut buffer).await);
                assert_eq!(frame.unpack::<CoeHeader>().unwrap().service(), CanService::SdoResponse);
                let header = frame.unpack::<SdoHeader>().unwrap();
                assert_eq!(SdoCommandResponse::try_from(header.command()).unwrap(), SdoCommandResponse::Download);
                assert_eq!(header.index(), sdo.index);
                assert_eq!(header.sub(), sdo.sub.unwrap());
            }
            
            // send many segments for the rest of the data, aknowledge each time
            let mut toggle = false;
            while data.remain().len() != 0 {
                // send segment
                {
                    let segment = data.remain().len().min(SDO_SEGMENT_MAX_SIZE);
                    let mut frame = Cursor::new(buffer.as_mut_slice());
                    frame.pack(&CoeHeader::new(u9::new(0), CanService::SdoRequest)).unwrap();
                    frame.pack(&SdoSegmentHeader::new(
                            data.remain().len() != 0, 
                            u3::new(0), 
                            toggle, 
                            u3::from(SdoCommandRequest::Download),
                        )).unwrap();
                    frame.write(data.read(segment).unwrap()).unwrap();
                    self.mailbox.write(MailboxType::Can, priority, frame.finish()).await;
                }
                
                // receive aknowledge
                {
                    // TODO check for possible SDO error
                    let mut frame = Cursor::new(self.mailbox.read(MailboxType::Can, priority, &mut buffer).await);
                    assert_eq!(frame.unpack::<CoeHeader>().unwrap().service(), CanService::SdoResponse);
                    let header = frame.unpack::<SdoSegmentHeader>().unwrap();
                    assert_eq!(SdoCommandResponse::try_from(header.command()).unwrap(), SdoCommandResponse::DownloadSegment);
                    assert_eq!(header.toggle(), toggle);
                    toggle = !toggle;
                    
                    // TODO: check for abort
                }
            }
		}
		
        // TODO: error propagation instead of asserts
        // TODO send SdoCommand::Abort in case any error
	}
	
	pub fn pdo_read() {todo!()}
	pub fn pdo_write() {todo!()}
	
	pub fn info_dictionnary() {todo!()}
	pub fn info_sdo() {todo!()}
	pub fn info_subitem() {todo!()}
}

// struct CoeFrame<'a> {
//     header: CoeHeader,
//     data: &'a [u8],
// }

#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct CoeHeader {
    /// present in the Can protocol, but not used in CoE
    number: u9,
    reserved: u3,
    /// Can command
    service: CanService,
}
data::bilge_pdudata!(CoeHeader, u16);

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
data::bilge_pdudata!(CanService, u4);


// use crate::data::FrameData;
// use core::marker::PhantomData;
// 
// struct SdoFrame<'a, T: FrameData<'a>> {
//     header: SdoHeader,
//     data: T,
//     phantom: PhantomData<&'a ()>,
// }
// 
// impl<'a, T: FrameData<'a>>   FrameData<'a> for SdoFrame<'a, T> {
//     fn pack(&self, dst: &mut [u8]) -> PackingResult<()> {
//         dst[.. SdoHeader::packed_size()].copy_from_slice(&self.header.pack());
//         self.data.pack(&mut dst[SdoHeader::packed_size() ..])?;
//         Ok(())
//     }
//     fn unpack(src: &'a [u8]) -> PackingResult<Self> {
// 		let header = SdoHeader::unpack(src)?;
//         Ok(Self {
//             header,
//             data: T::unpack(&src[SdoHeader::packed_size() ..][.. header.length() as usize])?,
//         })
//     }
// }


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
    /// operation to perform with the indexed SDO, this should be a value of [SdoCommandRequest] or [SdoCommandResponse]
    command: u3,
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
data::bilge_pdudata!(SdoHeader, u32);

// struct SdoSegmentFrame<'a> {
//     header: SdoSegmentHeader,
//     data: &'a [u8],
// }

#[bitsize(8)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct SdoSegmentHeader {
    more: bool,
    size: u3,
    toggle: bool,
    command: u3,
}
data::bilge_pdudata!(SdoSegmentHeader, u8);

/// request operation to perform with an SDO in CoE
///
/// ETG.1000.6 5.6.2.1-7
#[bitsize(3)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
enum SdoCommandRequest {
    Download = 0x1,
    DownloadSegment = 0x0,
    Upload = 0x2,
    UploadSegment = 0x3,
    Abort = 0x4,
}
data::bilge_pdudata!(SdoCommandRequest, u3);

/// response operation to perform with an SDO in CoE
///
/// ETG.1000.6 5.6.2.1-7
#[bitsize(3)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
enum SdoCommandResponse {
    Download = 0x3,
    DownloadSegment = 0x1,
    Upload = 0x2,
    UploadSegment = 0x0,
    Abort = 0x4,
}
data::bilge_pdudata!(SdoCommandResponse, u3);


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

