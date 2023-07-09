/*!
    SII (Slave Information Interface) allows to retreive declarative informations about a slave (like a manifest) like product code, vendor, etc as well as slave boot-up configs
    
    This module expose the standard EEPROM registers. registers are defined as [Field]s in the EEPROM, which content can be accessed using the instance of [Sii] (Slave Information Interface) proper to each slave.

    ETG.1000.6 5.4
*/

use crate::{
    rawmaster::{RawMaster, SlaveAddress},
    data::{self, PduData, Storage, Field, Cursor},
    registers,
    };
use bilge::prelude::*;

const WORD: usize = core::mem::size_of::<u16>();


//  ETG.1000.6 5.4 table 16

/// Initialization value for PDI Control register (0x140 - 0x141)
pub const pdi_control: Field<u16> = Field::simple(WORD*0x0000);
/// Initialization value for PDI Configuration register (0x150-0x151)
pub const pdi_config: Field<u16> = Field::simple(WORD*0x0001);
/// Sync Impulse in multiples of 10 ns
pub const sync_impulse: Field<u16> = Field::simple(WORD*0x0002);
/// intialization value for PDI Configuration register R8 most significant word (0x152-0x153
pub const pdi_config2: Field<u16> = Field::simple(WORD*0x0003);
/// Alias Address
pub const address_alias: Field<u16> = Field::simple(WORD*0x0004);
/// low byte contains remainder of division of word 0 to word 6 as unsigned number divided by the polynomial x^8+x^2+x+1(initial value 0xFF)
pub const checksum: Field<u16> = Field::simple(WORD*0x0007);

pub mod device {
    use super::*;
    
    pub const vendor: Field<u32> = Field::simple(WORD*0x0008);
    pub const product: Field<u32> = Field::simple(WORD*0x000a);
    pub const revision: Field<u32> = Field::simple(WORD*0x000c);
    pub const serial_number: Field<u32> = Field::simple(WORD*0x000e);
}

pub mod mailbox {
    use super::*;
    
    /// mailbox recommended parameters during [crate::SlaveState::Boostrap] state
    pub mod boostrap {
        use super::*;
        pub mod receive {
            use super::*;
    
            /// Send Mailbox Offset for Bootstrap state (slave to master)
            pub const offset: Field<u16> = Field::simple(WORD*0x0014);
            /// Send Mailbox Size for Bootstrap state (slave to master)
            /// Standard Mailbox size and Bootstrap Mailbox can differ. A bigger Mailbox size in Bootstrap mode can be used for optimiziation
            pub const size: Field<u16> = Field::simple(WORD*0x0015);
        }
       pub  mod send {
            use super::*;
    
            /// Receive Mailbox Offset for Standard state (master to slave)
            pub const offset: Field<u16> = Field::simple(WORD*0x0016);
            /// Receive Mailbox Size for Standard state (master to slave)
            pub const size: Field<u16> = Field::simple(WORD*0x0017);
        }
    }
    /// mailbox recommended parameters during other slave states
    pub mod standard {
        use super::*;
        pub mod receive {
            use super::*;
    
            /// Receive Mailbox Offset for Standard state (master to slave)
            pub const offset: Field<u16> = Field::simple(WORD*0x0018);
            /// Receive Mailbox Size for Standard state (master to slave)
            pub const size: Field<u16> = Field::simple(WORD*0x0019);
        }
        pub mod send {
            use super::*;
    
            /// Send Mailbox Offset for Standard state (slave to master)
            pub const offset: Field<u16> = Field::simple(WORD*0x001a);
            /// Send Mailbox Size for Standard state (slave to master)
            pub const size: Field<u16> = Field::simple(WORD*0x001b);
        }
    }
    /// Mailbox Protocols Supported as defined in ETG.1000.6 Table 18
    pub const protocols: Field<MailboxTypes> = Field::simple(WORD*0x001c);
}

/**
    size of EEPROM in [KiBit] + 1
    NOTE: KiBit means 1024 Bit.
    NOTE: size = 0 means a EEPROM size of 1 KiBit
*/
pub const eeprom_size: Field<u16> = Field::simple(WORD*0x003e);
/// This Version is 1
pub const version: Field<u16> = Field::simple(WORD*0x003f);




/// implementation of the Slave Information Interface (SII) to communicate with a slave's EEPROM memory
pub struct Sii<'a> {
    master: &'a RawMaster,
    slave: SlaveAddress,
    /// address unit (number of bytes) to use for communication
    unit: u16,
    /// whether the EEPROM is writable through the SII
    writable: bool,
}
impl<'a> Sii<'a> {
    pub async fn new(master: &'a RawMaster, slave: SlaveAddress) -> Sii<'a> {
        let status = master.read(slave, registers::sii::control).await.one();
        let unit = match status.address_unit() {
            registers::SiiUnit::Byte => 1,
            registers::SiiUnit::Word => 2,
        };
        assert!(!status.checksum_error(), "corrupted slave information EEPROM");
        Self {master, slave, unit, writable: status.write_access()}
        // TODO: error propagation
    }
    /// tells if the EEPROM is writable through the SII
    pub fn writable(&self) -> bool {self.writable}
    
    /// read data from the slave's EEPROM using the SII
    pub async fn read<T: PduData>(&mut self, field: Field<T>) -> T {
        let mut buffer = T::Packed::uninit();
        
        let mut cursor = Cursor::new(buffer.as_mut());
        while cursor.remain().len() != 0 {
            // send request
            self.master.write(self.slave, registers::sii::control_address, registers::SiiControlAddress {
                control: {
                    let mut control = registers::SiiControl::default();
                    control.set_read_operation(true);
                    control
                },
                address: ((field.byte + cursor.position()) as u16) / self.unit,
            }).await.one();
            
            // wait for interface to become available
            let status = loop {
                let answer = self.master.read(self.slave, registers::sii::control).await;
                if answer.answers == 1 && ! answer.value.busy()  && ! answer.value.read_operation()
                    {break answer.value}
            };
            // check for errors
            assert!(!status.command_error() && !status.device_info_error());
            // buffer the result
            let size = match status.read_size() {
                registers::SiiTransaction::Bytes4 => 4,
                registers::SiiTransaction::Bytes8 => 8,
                };
            cursor.write(&self.master.read(self.slave, registers::sii::data).await
                            .value[.. size]).unwrap();
        }
        T::unpack(buffer.as_ref()).unwrap()
        // TODO: error propagation
    }
    
    /// write data to the slave's EEPROM using the SII
    pub async fn write<T: PduData>(&mut self, field: Field<T>, value: &T) {
        let mut buffer = T::Packed::uninit();
        value.pack(buffer.as_mut()).unwrap();
        
        let mut cursor = Cursor::new(buffer.as_mut());
        while cursor.remain().len() != 0 {
            // write operation is forced to be 2 bytes (ETG.1000.4 6.4.5)
            // send request
            self.master.write(self.slave, registers::sii::control_address_data, registers::SiiControlAddressData {
                control: {
                    let mut control = registers::SiiControl::default();
                    control.set_write_operation(true);
                    control
                },
                address: ((field.byte + cursor.position()) as u16) / self.unit,
                reserved: 0,
                data: cursor.unpack().unwrap(),
            }).await.one();
            
            // wait for interface to become available
            let status = loop {
                let answer = self.master.read(self.slave, registers::sii::control).await;
                if answer.answers == 1 && ! answer.value.busy()  && ! answer.value.write_operation()
                    {break answer.value}
            };
            // check for errors
            assert!(!status.command_error() && !status.write_error());
        }
        // TODO: error propagation
    }
    
    /// reload first 128 bits of data from the EEPROM
    pub async fn reload(&mut self) {
        self.master.write(self.slave, registers::sii::control, {
            let mut control = registers::SiiControl::default();
            control.set_reload_operation(true);
            control
        }).await.one();
        
        // wait for interface to become available
        let status = loop {
            let answer = self.master.read(self.slave, registers::sii::control).await;
            if answer.answers == 1 && ! answer.value.busy() && ! answer.value.reload_operation()  
                {break answer.value}
        };
        // check for errors
        assert!(!status.command_error() && !status.checksum_error() && !status.device_info_error());
        // TODO: error propagation
    }
}

pub struct SiiIterator<'a> {
    sii: &'a mut Sii<'a>,
    position: usize,
}
impl SiiIterator<'_> {
    pub async fn next(&mut self) -> Option<CategoryHeader> {
        let header = self.sii.read(Field::<CategoryHeader>::simple(self.position)).await;
        self.position += usize::from(header.size());
        Some(header)
    }
}



/**
    header for a SII category
    
    ETG.1000.6 table 17
*/
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct CategoryHeader {
    /// Category Type as defined in ETG.1000.6 Table 19
    pub category: CategoryType,
    /// Following Category Word Size x
    pub size: u16,
}
data::bilge_pdudata!(CategoryHeader, u32);

/**
    type of category in the SII

    ETG.1000.6 table 19
*/
#[bitsize(16)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
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

/// ETG.1000.6 table 18
#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct MailboxTypes {
    /// ADS over EtherCAT (routing and parallel services)
    pub ads: bool,
    /// Ethernet over EtherCAT (tunnelling of Data Link services)
    pub ethernet: bool,
    /// CAN application protocol over EtherCAT (access to SDO)
    pub can: bool,
    /// File Access over EtherCAT
    pub file: bool,
    /// Servo Drive Profile over EtherCAT
    pub servo: bool,
    /// Vendor specific protocol over EtherCAT
    pub specific: bool,
    reserved: u2,
}
data::bilge_pdudata!(MailboxTypes, u8);

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
    _reserved: u7,
}
#[bitsize(8)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct EoeDetails {
    // protocol supported
    pub enable: bool,
    _reserved: u7,
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
    _reserved: u3,
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
