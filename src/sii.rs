/*!
    SII (Slave Information Interface) allows to retreive declarative informations about a slave (like a manifest) like product code, vendor, etc as well as slave boot-up configs

    ETG.1000.4.6.6.4
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




/// implementation of the Slave Information Interface (SII) to communicate with a slave's EEPROM memory
pub struct Sii {
    master: Arc<RawMaster>,
    slave: SlaveAddress,
    /// address unit (number of bytes) to use for communication
    unit: u16,
    /// whether the EEPROM is writable through the SII
    writable: bool,
}
impl Sii {
    pub async fn new(master: Arc<RawMaster>, slave: SlaveAddress) -> EthercatResult<Sii, SiiError> {
        let status = master.read(slave, registers::sii::control).await.one()?;
        let unit = match status.address_unit() {
            registers::SiiUnit::Byte => 1,
            registers::SiiUnit::Word => 2,
        };
        if status.checksum_error()
            {return Err(EthercatError::Slave(slave, SiiError::Checksum))};
        Ok(Self {master, slave, unit, writable: status.write_access()})
    }
    /// tells if the EEPROM is writable through the SII
    pub fn writable(&self) -> bool {self.writable}
    
    /// read data from the slave's EEPROM using the SII
    pub async fn read<T: PduData>(&mut self, field: Field<T>) -> EthercatResult<T, SiiError> {
        let mut buffer = T::Packed::uninit();
        self.read_slice(field.byte as _, buffer.as_mut()).await?;
        Ok(T::unpack(buffer.as_ref())?)
    }
    pub async fn read_slice(&mut self, address: u16, value: &mut [u8]) -> EthercatResult<(), SiiError> {
        assert!(address % self.unit == 0, "address must be aligned (to 2 or 4 bytes depending on slave)");
        
        let mut cursor = Cursor::new(value);
        while cursor.remain().len() != 0 {
            // send request
            self.master.write(self.slave, registers::sii::control_address, registers::SiiControlAddress {
                control: {
                    let mut control = registers::SiiControl::default();
                    control.set_read_operation(true);
                    control
                },
                address: (address + cursor.position() as u16) / self.unit,
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
                };
            cursor.write(&self.master.read(self.slave, registers::sii::data).await.one()?[.. size]).unwrap();
        }
        Ok(())
    }

    /// write data to the slave's EEPROM using the SII
    pub async fn write<T: PduData>(&mut self, field: Field<T>, value: &T) -> EthercatResult<(), SiiError> {
        let mut buffer = T::Packed::uninit();
        value.pack(buffer.as_mut()).unwrap();
        self.write_slice(field.byte as _, buffer.as_ref()).await
    }
    pub async fn write_slice(&mut self, address: u16, value: &[u8]) -> EthercatResult<(), SiiError> {
        assert!(address % self.unit == 0, "address must be aligned (to 2 or 4 bytes depending on slave)");
        
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
                address: (address + cursor.position() as u16) / self.unit,
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
}

/**
    helper for parsing the category of the eeprom through the SII
*/
pub struct SiiCursor<'a> {
    sii: &'a mut Sii,
    position: u16,
}
impl<'a> SiiCursor<'a> {
    /// initialize at the given byte position in the EEPROM
    pub fn new(sii: &'a mut Sii, position: u16) -> Self 
        {Self {sii, position}}
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
    pub category: CategoryType,
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
    TxPdo = 50,
    /// RxPDO description structure of this category data see ETG.1000.6 Table 25
    RxPdo = 51,
    /// Distributed Clock for future use
    Dc = 60,
    #[fallback]
    Specific = 0x0800,
    /// mark the end of SII categories
    End = 0xffff,
}

/// ETG.1000.6 table 21
#[repr(packed)]
pub struct CategoryGeneral {
    /// Group Information (Vendor specific) - Index to STRINGS
    pub group: u8,
    /// Image Name (Vendor specific) - Index to STRINGS
    pub img: u8,
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

/// ETG.1000.6 table 25
struct CategoryPdo {
    // TODO
}

/// ETG.1000.6 table 26
struct CategoryPdoentry {
    // TODO
}
