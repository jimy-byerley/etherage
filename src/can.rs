//! implementation of CoE (Canopen Over Ethercat)

use crate::{
    mailbox::{Mailbox, MailboxType, MailboxError, MailboxHeader},
    sdo::Sdo,
    data::{self, PduData, Storage, Cursor},
    error::{EthercatError, EthercatResult},
    };
use bilge::prelude::*;
use tokio::sync::Mutex;
use std::sync::Arc;



const MAILBOX_MAX_SIZE: usize = 1024;
/// maximum byte size of sdo data that can be expedited
const EXPEDITED_MAX_SIZE: usize = 4;
/// maximum byte size of an sdo data that can be put in a sdo segment
/// it is constrained by the mailbox buffer size on the slave
const SDO_SEGMENT_MAX_SIZE: usize = MAILBOX_MAX_SIZE
                                        - <MailboxHeader as PduData>::Packed::LEN
                                        - <CoeHeader as PduData>::Packed::LEN
                                        - <SdoSegmentHeader as PduData>::Packed::LEN;

/**
    implementation of CoE (Canopen Over Ethercat)

    It works exactly as in a Can bus, except each of its frame is encapsulated in an ethercat mailbox frame, and PDOs access is therefore not realtime.
    For realtime PDOs exchange, they must be mapped to the logical memory using a SM (Sync Manager) channel.

    Canopen protocol exposes 2 data structures:

    - a dictionnary of simple values or single level structures, for non-realtime access

        these are named SDO (Service Data Object).
        See [crate::sdo] for more details

    - several buffers gathering dictionnary objects for realtime access

        these are named PDO (Process Data Object)

    The following shows how the mapping of SDOs to PDOs is done, how it extends to logical memory in the case of CoE, and how the master interacts with each memory area.
    
    ![CoE mapping](https://raw.githubusercontent.com/jimy-byerley/etherage/master/schemes/coe-mapping.svg)
    
    This scheme comes in addition to the slave memory areas described in [crate::rawmaster::RawMaster], for slaves supporting CoE.

    A `Can` instance is generally obtained from [Slave::coe](crate::Slave::coe)
*/
pub struct Can {
    mailbox: Arc<Mutex<Mailbox>>,
}
impl Can {
    pub fn new(mailbox: Arc<Mutex<Mailbox>>) -> Can {
        Can {mailbox}
    }
    /// read an SDO, any size
    pub async fn sdo_read<T: PduData>(&mut self, sdo: &Sdo<T>, priority: u2) 
        -> EthercatResult<T, CanError> 
    {
        let mut data = T::Packed::uninit();
        Ok(T::unpack(self.sdo_read_slice(&sdo.downcast(), priority, data.as_mut()).await?)?)
    }
        
    pub async fn sdo_read_slice<'b>(&mut self, sdo: &Sdo, priority: u2, data: &'b mut [u8]) 
        -> EthercatResult<&'b mut [u8], CanError>   
    {
        let mut mailbox = self.mailbox.lock().await;
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
        frame.write(&[0; 4]).unwrap();
        mailbox.write(MailboxType::Can, priority, frame.finish()).await?;
        
        // receive data
        let (header, frame) = Self::receive_sdo_response(
                &mut mailbox,
                &mut buffer, 
                SdoCommandResponse::Upload, 
                sdo,
                ).await?;
        if ! header.sized()
            {return Err(Error::Protocol("got SDO response without data size"))}
        
        
        if header.expedited() {
            // expedited transfer
            let total = EXPEDITED_MAX_SIZE - u8::from(header.size()) as usize;
            if total > data.len() 
                {return Err(Error::Master("data buffer is too small for requested SDO"))}
            data[.. total].copy_from_slice(Cursor::new(frame)
                .read(total)
                .map_err(|_| Error::Protocol("inconsistent expedited response data size"))?
                );
            Ok(&mut data[.. total])
        }
        else {
            // normal transfer, eventually segmented
            let mut frame = Cursor::new(frame);
            let total = frame.unpack::<u32>()
                .map_err(|_| Error::Protocol("unable unpack sdo size from SDO response"))?
                .try_into().expect("SDO is too big for master memory");
            if total > data.len()
                {return Err(Error::Master("read buffer is too small for requested SDO"))}
            
            let mut received = Cursor::new(&mut data.as_mut()[.. total]);
            let mut toggle = false;
            received.write(frame.remain())
                .map_err(|_| Error::Protocol("received more data than declared from SDO"))?;
            
            // receive more data from segments
            // TODO check for possible SDO error
            while received.remain().len() != 0 {
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
                    frame.write(&[0; 7]).unwrap();
                    mailbox.write(MailboxType::Can, priority, frame.finish()).await?;
                }

                // receive segment
                {
                    let (header, segment) = Self::receive_sdo_segment(
                            &mut mailbox, 
                            &mut buffer, 
                            SdoCommandResponse::UploadSegment, 
                            toggle,
                            ).await?;
                    let segment = &segment[.. received.remain().len()];
                    received.write(segment)
                        .map_err(|_| Error::Protocol("received more data than declared from SDO"))?;
                    
                    if ! header.more () {break}
                }

                toggle = ! toggle;
            }
            Ok(received.finish())
        }

        // TODO send SdoCommand::Abort in case any error
    }
    /// write an SDO, any size
    pub async fn sdo_write<T: PduData>(&mut self, sdo: &Sdo<T>, priority: u2, data: T) 
        -> EthercatResult<(), CanError>  
    {
        let mut packed = T::Packed::uninit();
        data.pack(packed.as_mut())
            .expect("unable to pack data for sending");
        self.sdo_write_slice(&sdo.downcast(), priority, packed.as_ref()).await
    }
    pub async fn sdo_write_slice(&mut self, sdo: &Sdo, priority: u2, data: &[u8]) 
        -> EthercatResult<(), CanError>  
    {
        let mut mailbox = self.mailbox.lock().await;		
        let mut buffer = [0; MAILBOX_MAX_SIZE];
        if data.len() <= EXPEDITED_MAX_SIZE {
            // expedited transfer
            // send data in the 4 bytes instead of data size
            {
                let mut frame = Cursor::new(buffer.as_mut_slice());
                frame.pack(&CoeHeader::new(u9::new(0), CanService::SdoRequest)).unwrap();
                frame.pack(&SdoHeader::new(
                            true,
                            true,
                            u2::new((EXPEDITED_MAX_SIZE - data.len()) as u8),
                            sdo.sub.is_complete(),
                            u3::from(SdoCommandRequest::Download),
                            sdo.index,
                            sdo.sub.unwrap(),
                        )).unwrap();
                frame.write(data).unwrap();
                frame.write(&[0; 4][data.len() ..]).unwrap();
                mailbox.write(MailboxType::Can, priority, frame.finish()).await?;
            }

            // receive acknowledge
            Self::receive_sdo_response(
                &mut mailbox,
                &mut buffer,
                SdoCommandResponse::Download,
                sdo,
                ).await?;
        }
        else {
            // normal transfer, eventually segmented
            let mut data = Cursor::new(data.as_ref());

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
                frame.pack(&(data.remain().len() as u32)).unwrap();
                let segment = data.remain().len().min(SDO_SEGMENT_MAX_SIZE);
                frame.write(data.read(segment).unwrap()).unwrap();
                mailbox.write(MailboxType::Can, priority, frame.finish()).await?;
            }

            // receive acknowledge
            Self::receive_sdo_response(
                &mut mailbox,
                &mut buffer, 
                SdoCommandResponse::Download, 
                sdo,
                ).await?;
            
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
                            u3::from(SdoCommandRequest::DownloadSegment),
                        )).unwrap();
                    frame.write(data.read(segment).unwrap()).unwrap();
                    mailbox.write(MailboxType::Can, priority, frame.finish()).await?;
                }

                // receive aknowledge
                Self::receive_sdo_segment(
                    &mut mailbox,
                    &mut buffer, 
                    SdoCommandResponse::DownloadSegment, 
                    toggle,
                    ).await?;
                toggle = !toggle;
            }
        }
        Ok(())
        
        // TODO send SdoCommand::Abort in case any error
    }

    /// read the mailbox, check for
    async fn receive_sdo_response<'b, T: PduData>(
        mailbox: &mut Mailbox,
        buffer: &'b mut [u8], 
        expected: SdoCommandResponse,
        sdo: &Sdo<T>, 
        ) -> EthercatResult<(SdoHeader, &'b [u8]), CanError> 
    {
        let mut frame = Cursor::new(mailbox.read(MailboxType::Can, buffer).await?);
        
        let check_header = |header: SdoHeader| {
            if header.index() != sdo.index        {return Err(Error::Protocol("slave answered about wrong item"))}
            if header.sub() != sdo.sub.unwrap()   {return Err(Error::Protocol("slave answered about wrong subitem"))}
            Ok(())
        };
        
        match frame.unpack::<CoeHeader>()
            .map_err(|_| Error::Protocol("unable to unpack COE frame header"))?
            .service() 
        {
            CanService::SdoResponse => {
                let header = frame.unpack::<SdoHeader>()
                    .map_err(|_| Error::Protocol("unable to unpack SDO response header"))?;
                if SdoCommandResponse::try_from(header.command()) != Ok(expected)
                    {return Err(Error::Protocol("slave answered with wrong operation"))}
                check_header(header)?;
                Ok((header, frame.remain()))
                },
            CanService::SdoRequest => {
                let header = frame.unpack::<SdoHeader>()
                    .map_err(|_| Error::Protocol("unable to unpack SDO request header"))?;
                if SdoCommandRequest::try_from(header.command()) != Ok(SdoCommandRequest::Abort)
                    {return Err(Error::Protocol("slave answered a COE request"))}
                check_header(header)?;
                let error = frame.unpack::<SdoAbortCode>()
                        .map_err(|_| Error::Protocol("unable to unpack SDO error code"))?;
                Err(Error::Slave(mailbox.slave(), CanError::Sdo(error)))
                },
            _ => {return Err(Error::Protocol("unexpected COE service during SDO operation"))},
        }
    }

    async fn receive_sdo_segment<'b>(
        mailbox: &mut Mailbox,
        buffer: &'b mut [u8], 
        expected: SdoCommandResponse,
        toggle: bool, 
        ) -> EthercatResult<(SdoSegmentHeader, &'b [u8]), CanError> 
    {
        let mut frame = Cursor::new(mailbox.read(MailboxType::Can, buffer).await?);
        
        match frame.unpack::<CoeHeader>()
            .map_err(|_|  Error::Protocol("unable to unpack COE frame header"))?
            .service() 
        {
            CanService::SdoResponse => {
                let header = frame.unpack::<SdoSegmentHeader>()
                    .map_err(|_|  Error::Protocol("unable to unpack segment response header"))?;
                if SdoCommandResponse::try_from(header.command()) != Ok(expected)
                    {return Err(Error::Protocol("slave answered with a COE request"))}
                if header.toggle() != toggle   
                    {return Err(Error::Protocol("bad toggle bit in segment received"))}
                Ok((header, frame.remain()))
                },
            CanService::SdoRequest => {
                let header = frame.unpack::<SdoHeader>()
                    .map_err(|_|  Error::Protocol("unable to unpack request header"))?;
                if SdoCommandRequest::try_from(header.command()) != Ok(SdoCommandRequest::Abort)
                    {return Err(Error::Protocol("slave answered a COE request"))}
                let error = frame.unpack::<SdoAbortCode>()
                        .map_err(|_| Error::Protocol("unable to unpack SDO error code"))?;
                Err(Error::Slave(mailbox.slave(), CanError::Sdo(error)))
                },
            _ => {return Err(Error::Protocol("unexpected COE service during SDO segment operation"))},
        }
    }

    pub fn pdo_read() {todo!()}
    pub fn pdo_write() {todo!()}

    pub fn info_dictionnary() {todo!()}
    pub fn info_sdo() {todo!()}
    pub fn info_subitem() {todo!()}
}



#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct CoeHeader {
    /// present in the Can protocol, but not used in CoE
    pub number: u9,
    reserved: u3,
    /// Can command
    pub service: CanService,
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
pub enum CanService {
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
pub struct SdoHeader {
    /// true if field `size` is used
    pub sized: bool,
    /// true in case of an expedited transfer (the data size specified by `size`)
    pub expedited: bool,
    /// indicate the data size but not as an integer.
    /// this value shall be `4 - data.len()`
    pub size: u2,
    /// true if a complete SDO is accessed
    pub complete: bool,
    /// operation to perform with the indexed SDO, this should be a value of [SdoCommandRequest] or [SdoCommandResponse]
    pub command: u3,
    /// SDO index
    pub index: u16,
    /**
    - if subitem is accessed: SDO subindex
    - if complete item is accessed:
        + put 0 to include subindex 0 in transmission
        + put 1 to exclude subindex 0 from transmission
    */
    pub sub: u8,
}
data::bilge_pdudata!(SdoHeader, u32);

#[bitsize(8)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct SdoSegmentHeader {
    pub more: bool,
    pub size: u3,
    pub toggle: bool,
    pub command: u3,
}
data::bilge_pdudata!(SdoSegmentHeader, u8);

/// request operation to perform with an SDO in CoE
///
/// ETG.1000.6 5.6.2.1-7
#[bitsize(3)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SdoCommandRequest {
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
pub enum SdoCommandResponse {
    Download = 0x3,
    DownloadSegment = 0x1,
    Upload = 0x2,
    UploadSegment = 0x0,
    Abort = 0x4,
}
data::bilge_pdudata!(SdoCommandResponse, u3);

#[bitsize(32)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SdoAbortCode {
    /// Toggle bit not changed
    BadToggle = 0x05_03_00_00,
    /// SDO protocol timeout
    Timeout = 0x05_04_00_00,
    /// Client/Server command specifier not valid or unknown
    UnsupportedCommand = 0x05_04_00_01,
    /// Out of memory
    OufOfMemory = 0x05_04_00_05,
    /// Unsupported access to an object, this is raised when trying to access a complete SDO when complete SDO access is not supported
    UnsupportedAccess = 0x06_01_00_00,
    /// Attempt to read to a write only object
    WriteOnly = 0x06_01_00_01,
    /// Attempt to write to a read only object
    ReadOnly = 0x06_01_00_02,
    /// Subindex cannot be written, SI0 must be 0 for write access
    WriteError = 0x06_01_00_03,
    /// SDO Complete access not supported for objects of variable length such as ENUM object types
    VariableLength = 0x06_01_00_04,
    /// Object length exceeds mailbox size
    ObjectTooBig = 0x06_01_00_05,
    /// Object mapped to RxPDO, SDO Download blocked
    LockedByPdo = 0x06_01_00_06,
    /// The object does not exist in the object directory
    InvalidIndex = 0x06_02_00_00,
    /// The object can not be mapped into the PDO
    CannotMap = 0x06_04_00_41,
    /// The number and length of the objects to be mapped would exceed the PDO length
    PdoTooSmall = 0x06_04_00_42,
    /// General parameter incompatibility reason
    IncompatibleParameter = 0x06_04_00_43,
    /// General internal incompatibility in the device
    IncompatibleDevice = 0x06_04_00_47,
    /// Access failed due to a hardware error
    HardwareError = 0x06_06_00_00,
    /// Data type does not match, length of service parameter does not match
    InvalidLength = 0x06_07_00_10,
    /// Data type does not match, length of service parameter too high
    ServiceTooBig = 0x06_07_00_12,
    /// Data type does not match, length of service parameter too low
    ServiceTooSmall = 0x06_07_00_13,
    /// Subindex does not exist
    InvalidSubIndex = 0x06_09_00_11,
    /// Value range of parameter exceeded (only for write access)
    ValueOutOfRange = 0x06_09_00_30,
    /// Value of parameter written too high
    ValueTooHigh = 0x06_09_00_31,
    /// Value of parameter written too low
    ValueTooLow = 0x06_09_00_32,
    /// Maximum value is less than minimum value
    InvalidRange = 0x06_09_00_36,
    /// General error
    GeneralError = 0x08_00_00_00,
    /**
    Data cannot be transferred or stored to the application

    NOTE: This is the general Abort Code in case no further detail on the reason can determined. It is recommended to use one of the more detailed Abort Codes (0x08000021, 0x08000022)
    */
    Refused = 0x08_00_00_20,
    /**
    Data cannot be transferred or stored to the application because of local control

    NOTE: “local control” means an application specific reason. It does not mean the
    ESM-specific control
    */
    ApplicationRefused = 0x08_00_00_21,
    /**
    Data cannot be transferred or stored to the application because of the present device state

    NOTE: “device state” means the ESM state
    */
    StateRefused = 0x08_00_00_22,
    /// Object dictionary dynamic generation fails or no object dictionary is present
    DictionnaryEmpty = 0x08_00_00_23,
}
data::bilge_pdudata!(SdoAbortCode, u32);

impl SdoAbortCode {
    pub fn object_related(self) -> bool   {u32::from(self) >> 24 == 0x06}
    pub fn subitem_related(self) -> bool  {u32::from(self) >> 16 == 0x06_09}
    pub fn mapping_related(self) -> bool  {u32::from(self) >> 16 == 0x06_04}
    pub fn device_related(self) -> bool   {u32::from(self) >> 24 == 0x08}
    pub fn protocol_related(self) -> bool {u32::from(self) >> 24 == 0x05}
}

/// error type returned by the CoE functions
type Error = EthercatError<CanError>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CanError {
    Mailbox(MailboxError),
    Sdo(SdoAbortCode),
//     Pdo(PdoAbortCode),
}
impl From<MailboxError> for CanError {
    fn from(src: MailboxError) -> Self {CanError::Mailbox(src)}
}
impl From<EthercatError<MailboxError>> for EthercatError<CanError> {
    fn from(src: EthercatError<MailboxError>) -> Self {src.into()}
}
impl From<EthercatError<()>> for EthercatError<CanError> {
    fn from(src: EthercatError<()>) -> Self {src.upgrade()}
}

