/*!
    SII (Slave Information Interface) allows to retreive from EEPROM declarative informations about a slave (like a manifest) like product code, vendor, etc as well as slave boot-up configs.

    The EEPROM can also store configurations variables for the specific slaves purpose (like control loop coefficients, safety settings, etc) but these values are vendor-specific and often now meant for user inspection through the SII. They are still accessible using the tools provided here, but not structure is provided to interpret them.

    ETG.1000.4.6.6.4, ETG.2010

    This module features [Sii] and [SiiCursor] in order to read/write and parse the eeprom data

    The EEPROM has 2 regions of data:

    - EEPROM registers: fixed addresses, described in [eeprom] and accessed by [Sii]
    - EEPROM categories: contiguous data blocks, described by the `Category*` structs here and accessed by [SiiCursor]

    Here is how to use registers:
    ```ignore
    sii.acquire().await?;
    let vendor = sii.read(eeprom::device::vendor).await?;
    let alias = sii.read(eeprom::address_alias).await?;
    sii.release().await?;
    ```

    Here is how to parse the categories:
    ```ignore
    sii.acquire().await?;
    let mut categories = sii.categories();
    let general = loop {
        let category = categories.unpack::<CategoryHeader>().await?;
        // we got our desired category, do something with it
        if category.ty() == CategoryType::General {
            // we can then read the category (or at least its header) as a register
            break Ok(categories.unpack::<CategoryGeneral>().await?)
        }
        // end of eeprom reached
        else if category.ty() == CategoryType::End {
            break Err(EthercatError::Master("no general category in EEPROM"))
        }
        // or squeeze the current category
        else {
            categories.advance(WORD*category.size());
        }
    };
    sii.release().await?;
    ```
*/

use crate::{
    error::{EthercatError, EthercatResult},
    data::{self, PduData, Storage, Field, Cursor},
    rawmaster::{RawMaster, SlaveAddress},
    registers,
    eeprom,
    };
use std::sync::Arc;
use bilge::prelude::*;


pub const WORD: u16 = eeprom::WORD as _;


/**
    implementation of the Slave Information Interface (SII) to communicate with a slave's EEPROM memory

    The EEPROM has 2 regions of data:

    - EEPROM registers: at fixed addresses, described in [eeprom]
    - EEPROM categories: as contiguous data blocks, starting from fixed address [eeprom::categories]

    This struct is providing access to both, but only works with fixed addresses. To parse the categories, use a cursor returned by [Self::categories] then parse the structures iteratively.

    A `Sii` instance is generally obtained from [Slave::sii](crate::Slave::sii)
*/
pub struct Sii {
    master: Arc<RawMaster>,
    slave: SlaveAddress,
    /// address mask (part of the address actually used by the slave)
    mask: u16,
    /// whether the EEPROM is writable through the SII
    writable: bool,
    /// whether the master owns access to the SII (and EEPROM)
    locked: bool,
}
impl Sii {
    /**
        create an instance of this struct to use the SII of the given slave

        to stay protocol-safe, one instance of this struct only should exist per slave.

        At contrary to the EEPROM addresses used at the ethercat communication level, this struct and its methods only use byte addresses, write requests should be word-aligned.
    */
    pub async fn new(master: Arc<RawMaster>, slave: SlaveAddress) -> EthercatResult<Sii, SiiError> {
        let status = master.read(slave, registers::sii::control).await.one()?;
        let mask = match status.address_unit() {
            registers::SiiUnit::Byte => 0xff,
            registers::SiiUnit::Word => 0xffff,
        };
        if status.checksum_error()
            {return Err(EthercatError::Slave(slave, SiiError::Checksum))};
        Ok(Self {
            master,
            slave,
            mask,
            writable: status.write_access(),
            locked: false,
            })
    }
    /// tells if the EEPROM is writable through the SII
    pub fn writable(&self) -> bool {self.writable}

    /**
        acquire the SII so we can use it through the registers

        The access to the EEPROM is always made through the SII, even internally for the slave, which mean for the slave to access its EEPROM, the master should not have it.

        the access need to be acquired by the master before it to read or write to the EEPROM. Any attempt without acquiring it will fail
    */
    pub async fn acquire(&mut self) -> EthercatResult {
        if ! self.locked {
            self.locked = true;
            self.master.write(self.slave, registers::sii::access, {
                let mut config = registers::SiiAccess::default();
                config.set_owner(registers::SiiOwner::EthercatDL);
                config
                }).await.one()?;
        }
        Ok(())
    }
    /**
        release the SII to the device internal control

        The access to the EEPROM usually needs to be released during slave initialization steps (meaning for state transitions). Most attempt to initialize without releasing will certainly fail.
    */
    pub async fn release(&mut self) -> EthercatResult {
        if self.locked {
            self.locked = false;
            self.master.write(self.slave, registers::sii::access, {
                let mut config = registers::SiiAccess::default();
                config.set_owner(registers::SiiOwner::Pdi);
                config
                }).await.one()?;
        }
        Ok(())
    }
    
    /// read data from the slave's EEPROM using the SII
    pub async fn read<T: PduData>(&mut self, field: Field<T>) -> EthercatResult<T, SiiError> {
        let mut buffer = T::Packed::uninit();
        self.read_slice(field.byte as _, buffer.as_mut()).await?;
        Ok(T::unpack(buffer.as_ref())?)
    }
    /// read a slice of the slave's EEPROM memory
    pub async fn read_slice<'b>(&mut self, address: u16, value: &'b mut [u8]) -> EthercatResult<&'b [u8], SiiError> {
        // some slaves use 2 byte addresses but declare they are using 1 only, so disable this check for now
//         if address & !self.mask != 0
//             {return Err(EthercatError::Master("wrong EEPROM address: address range is 1 byte only"))}

        let mut start = (address % WORD) as usize;
        let mut cursor = Cursor::new(value.as_mut());
        while cursor.remain().len() != 0 {
            // send request
            self.master.write(self.slave, registers::sii::control_address, registers::SiiControlAddress {
                control: {
                    let mut control = registers::SiiControl::default();
                    control.set_read_operation(true);
                    control
                },
                address: (address + cursor.position() as u16) / WORD,
            }).await.one()?;
            // wait for interface to become available
            let status = loop {
                if let Ok(answer) = self.master.read(self.slave, registers::sii::control).await.one() {
                    if ! answer.busy()  && ! answer.read_operation()
                        {break answer}
                }
            };
            // check for errors
            if status.command_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::Command))}
            if status.device_info_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::DeviceInfo))}

            // buffer the result
            let size = match status.read_size() {
                registers::SiiTransaction::Bytes4 => 4,
                registers::SiiTransaction::Bytes8 => 8,
                }.min(start + cursor.remain().len());
            let data = self.master.read(self.slave, registers::sii::data).await.one()?;
            cursor.write(&data[start .. size]).unwrap();
            start = 0;
        }
        Ok(value)
    }

    /**
        write data to the slave's EEPROM using the SII

        this will only work if [Self::writable], else will raise an error
    */
    pub async fn write<T: PduData>(&mut self, field: Field<T>, value: &T) -> EthercatResult<(), SiiError> {
        let mut buffer = T::Packed::uninit();
        value.pack(buffer.as_mut()).unwrap();
        self.write_slice(field.byte as _, buffer.as_ref()).await
    }
    /**
        write the given slice of data in the slave's EEPROM

        this will only work if [Self::writable], else will raise an error
    */
    pub async fn write_slice(&mut self, address: u16, value: &[u8]) -> EthercatResult<(), SiiError> {
        if address % WORD != 0
            {return Err(EthercatError::Master("address must be word-aligned"))}
        // some slaves use 2 byte addresses but declare they are using 1 only, so disable this check for now
//         if address & !self.mask != 0
//             {return Err(EthercatError::Master("wrong EEPROM address: address range is 1 byte only"))}

        let mut cursor = Cursor::new(value.as_ref());
        while cursor.remain().len() != 0 {
            // write operation is forced to be 2 bytes (ETG.1000.4 6.4.5)
            // send request
            self.master.write(self.slave, registers::sii::control_address_data, registers::SiiControlAddressData {
                control: {
                    let mut control = registers::SiiControl::default();
                    control.set_write_operation(true);
                    control
                },
                address: (address + cursor.position() as u16) / WORD,
                reserved: 0,
                data: cursor.unpack().unwrap(),
            }).await.one()?;

            // wait for interface to become available
            let status = loop {
                if let Ok(answer) = self.master.read(self.slave, registers::sii::control).await.one() {
                    if ! answer.busy()  && ! answer.write_operation()
                        {break answer}
                }
            };
            // check for errors
            if status.command_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::Command))}
            if status.write_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::Write))}
        }
        Ok(())
    }

    /// reload first 128 bits of data from the EEPROM
    pub async fn reload(&mut self) -> EthercatResult<(), SiiError> {
        self.master.write(self.slave, registers::sii::control, {
            let mut control = registers::SiiControl::default();
            control.set_reload_operation(true);
            control
        }).await.one()?;

        // wait for interface to become available
        let status = loop {
            if let Ok(answer) = self.master.read(self.slave, registers::sii::control).await.one() {
                if ! answer.busy() && ! answer.reload_operation()
                    {break answer}
            }
        };
        // check for errors
        if status.command_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::Command))}
        if status.checksum_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::Checksum))}
        if status.device_info_error()
                {return Err(EthercatError::Slave(self.slave, SiiError::DeviceInfo))}
        Ok(())
    }
    
    /// cursor pointing at the start of categories. See [CategoryHeader]
    pub fn categories(&mut self) -> SiiCursor<'_> {
        SiiCursor {
            sii: self,
            position: eeprom::categories,
            }
    }

    /**
        read the strings category of the EEPROM, and return its content as rust datatype

        these strings are usually pointed to by sdo values or other fields in the EEPROM's categories
    */
    pub async fn strings(&mut self) -> EthercatResult<Vec<String>, SiiError> {
        let mut categories = self.categories();
        loop {
            let category = categories.unpack::<CategoryHeader>().await?;
            if category.ty() == CategoryType::Strings {
                let num = categories.unpack::<u8>().await?;
                let mut strings = Vec::with_capacity(num as _);

                for _ in 0 .. num {
                    // string length in byte
                    let len = categories.unpack::<u8>().await?;
                    // read string
                    let mut buffer = vec![0; len as _];
                    categories.read(&mut buffer).await?;
                    strings.push(String::from_utf8(buffer)
                        .map_err(|_|  EthercatError::<SiiError>::Master("strings in EEPROM are not UTF8"))?
                        );
                }

                return Ok(strings)
            }
            else if category.ty() == CategoryType::End {
                return Err(EthercatError::Master("no strings category in EEPROM"))
            }
            else {
                categories.advance(WORD*category.size());
            }
        }
    }

    /// readthe general informations category of the EEPROM and return its content
    pub async fn generals(&mut self) -> EthercatResult<CategoryGeneral, SiiError> {
        let mut categories = self.categories();
        loop {
            let category = categories.unpack::<CategoryHeader>().await?;
            if category.ty() == CategoryType::General {
                return categories.unpack::<CategoryGeneral>().await
            }
            else if category.ty() == CategoryType::End {
                return Err(EthercatError::Master("no general category in EEPROM"))
            }
            else {
                categories.advance(WORD*category.size());
            }
        }
    }
}

/**
    Helper for parsing the category of the eeprom through the SII

    It is inspired from [data::Cursor] but this one directly reads into the EEPROM rather than in a local buffer
*/
pub struct SiiCursor<'a> {
    sii: &'a mut Sii,
    position: u16,
}
impl<'a> SiiCursor<'a> {
    /// initialize at the given byte position in the EEPROM
    pub fn new(sii: &'a mut Sii, position: u16) -> Self 
        {Self {sii, position}}
    /// byte position in the EEPROM
    pub fn position(&self) -> u16
        {self.position}

    /// create a new instance of cursor at the same location, it is only meant to ease practice of parsing multiple time the same region
    pub fn shadow(&mut self) -> SiiCursor<'_> {
        SiiCursor {
            sii: self.sii,
            position: self.position,
            }
    }
    /// advance byte position of the given increment
    pub fn advance(&mut self, increment: u16) {
        self.position += increment;
    }
    /// read bytes filling the given slice and advance the position
    pub async fn read(&mut self, dst: &mut [u8]) -> EthercatResult<(), SiiError> {
        self.sii.read_slice(self.position, dst).await?;
        self.position += dst.len() as u16;
        Ok(())
    }
    /// read the given data and advance the position
    pub async fn unpack<T: PduData>(&mut self) -> EthercatResult<T, SiiError> {
        let mut buffer = T::Packed::uninit();
        self.read(buffer.as_mut()).await?;
        Ok(T::unpack(buffer.as_ref())?)
    }
    /// write the given bytes and advance the position
    pub async fn write(&mut self, dst: &[u8]) -> EthercatResult<(), SiiError> {
        self.sii.write_slice(self.position, dst).await?;
        self.position += dst.len() as u16;
        Ok(())
    }
    /// write the given data and advance the position
    pub async fn pack<T: PduData>(&mut self, value: T) -> EthercatResult<(), SiiError> {
        let mut buffer = T::Packed::uninit();
        value.pack(buffer.as_mut()).unwrap();
        self.write(buffer.as_ref()).await
    }
}

/// error raised by the SII of a slave
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SiiError {
    /// bad SII command
    Command,
    /// EEPROM data has been corrupted
    Checksum,
    /// bad data in device info section
    DeviceInfo,
    /// cannot write the requested location in EEPROM
    Write,
}

impl From<EthercatError<()>> for EthercatError<SiiError> {
    fn from(src: EthercatError<()>) -> Self {src.upgrade()}
}


/**
    header for a SII category
    
    ETG.1000.6 table 17
*/
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct CategoryHeader {
    /// category type as defined in ETG.1000.6 Table 19
    pub ty: CategoryType,
    /// size in word of the category
    pub size: u16,
}
data::bilge_pdudata!(CategoryHeader, u32);

/**
    type of category in the SII

    ETG.1000.6 table 19
*/
#[bitsize(16)]
#[derive(FromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum CategoryType {
    Nop = 0,
    /// String repository for other Categories structure of this category data see ETG.1000.6 Table 20
    Strings = 10,
    /// Data Types for future use
    DataTypes = 20,
    /// General information structure of this category data see ETG.1000.6 Table 21
    General = 30,
    /// FMMUs to be used structure of this category data see ETG.1000.6 Table 23
    Fmmu = 40,
    /// Sync Manager Configuration structure of this category data see ETG.1000.6 Table 24
    SyncManager = 41,
    /// TxPDO description structure of this category data see ETG.1000.6 Table 25
    PdoWrite = 50,
    /// RxPDO description structure of this category data see ETG.1000.6 Table 25
    PdoRead = 51,
    /// Distributed Clock for future use
    Dc = 60,
    #[fallback]
    Unsupported = 0x0800,
    /// mark the end of SII categories
    End = 0xffff,
}

/// ETG.1000.6 table 21
#[repr(packed)]
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CategoryGeneral {
    /// Group Information (Vendor specific) - Index to STRINGS
    pub group: u8,
    /// Image Name (Vendor specific) - Index to STRINGS
    pub image: u8,
    /// Device Order Number (Vendor specific) - Index to STRINGS
    pub order: u8,
    /// Device Name Information (Vendor specific) - Index to STRINGS
    pub name: u8,
    _reserved0: u8,
    /// supported CoE features
    pub coe: CoeDetails,
    /// supported FoE features
    pub foe: FoeDetails,
    /// supported EoE features
    pub eoe: EoeDetails,
    _reserved1: [u8;3],
    pub flags: GeneralFlags,
    /// EBus Current Consumption in mA, negative Values means feeding in current feed in sets the available current value to the given value
    pub ebus_current: i16,
    /// Index to Strings – duplicate for compatibility reasons
    pub group2: u8,
    _reserved2: u8,
    /// Description of Physical Ports
    pub ports: PhysicialPorts,
    /// Element defines the ESC memory address where the Identification ID is saved if Identification Method = IdentPhyM
    pub identification_address: u16,
}
data::packed_pdudata!(CategoryGeneral);

/// supported CoE features
#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct CoeDetails {
    pub sdo: bool,
    pub sdo_info: bool,
    pub pdo_assign: bool,
    pub pdo_config: bool,
    pub startup_upload: bool,
    pub sdo_complete: bool,
    _reserved: u2,
}
#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct FoeDetails {
    // protocol supported
    pub enable: bool,
    reserved: u7,
}
#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct EoeDetails {
    // protocol supported
    pub enable: bool,
    reserved: u7,
}
#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct GeneralFlags {
    pub enable_safeop: bool,
    pub enable_notlrw: bool,
    pub mbox_dll: bool,
    /// ID selector mirrored in AL Statud Code
    pub ident_alsts: bool,
    /// ID selector value mirrored in specific physical memory as deonted by the parameter “Physical Memory Address”
    pub ident_phym: bool,
    reserved: u3,
}

#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct PhysicialPorts {
    pub ports: [PhysicalPort; 4],
}
#[bitsize(4)]
#[derive(FromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum PhysicalPort {
    #[fallback]
    Disabled = 0x0,
    /// media independent interface
    Mii = 0x1,
    Reserved = 0x2,
    Ebus = 0x3,
    /// NOTE: Fast Hot Connect means a Port with Ethernet Physical Layer and Autonegotiation off (100Mbps fullduplex)
    FastHotconnect = 0x4,
}

/// ETG.1000.6 table 23
#[bitsize(8)]
#[derive(FromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum FmmuUsage {
    #[fallback]
    Disabled = 0,
    Outputs = 1,
    Inputs = 2,
    SyncManagerStatus = 3,
}

/// ETG.1000.6 table 24
#[bitsize(64)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct CategorySyncManager {
    /// Origin of Data (see Physical Start Address of SyncM)
    pub address: u16,
    pub length: u16,
    /// Defines Mode of Operation (see Control Register of SyncM)
    pub control: u8,
    /// don't care
    pub status: u8,
    pub enable: SyncManagerEnable,
    pub usage: SyncManagerUsage,
}
data::bilge_pdudata!(CategorySyncManager, u64);

#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct SyncManagerEnable {
    pub enable: bool,
    /// fixed content (info for config tool –SyncMan has fixed content)
    pub fixed_content: bool,
    /// virtual SyncManager (virtual SyncMan – no hardware resource used)
    pub virtual_sync_manager: bool,
    /// opOnly (SyncMan should be enabled only in OP state)
    pub oponly: bool,
    _reserved: u4,
}

#[bitsize(8)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SyncManagerUsage {
    Disabled = 0x0,
    MailboxOut = 0x1,
    MailboxIn = 0x2,
    ProcessOut = 0x3,
    ProcessIn = 0x4,
}

/**
    header for category describing a reading or writing PDO

    ETG.1000.6 table 25
*/
#[repr(packed)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CategoryPdo {
    /// index of SDO configuring the PDO
    index: u16,
    /// number of entries in the PDO
    entries: u8,
    /// reference to DC-sync
    synchronization: u8,
    /// name of the PDO object (reference to category strings)
    name: u8,
    /// for future use
    flags: u16,
}
data::packed_pdudata!(CategoryPdo);

/**
    structure describing an entry in a PDO

    ETG.1000.6 table 26
*/
#[repr(packed)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CategoryPdoentry {
    /// index of the SDO
    index: u16,
    /// index of field in the SDO (or 0 if complete SDO)
    sub: u8,
    /// name of this SDO
    name: u8,
    /// data type of the entry
    dtype: u8,
    /// data length of the entry
    bitlen: u8,
    /// for future use
    flags: u16,
}
data::packed_pdudata!(CategoryPdoentry);
