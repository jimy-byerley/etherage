/*!
    ETG.1000.6 5.4
*/

use crate::data::Field;

const WORD: usize = core::mem::size_of<T>(u16);


//  ETG.1000.6 5.4 table 16

/// Initialization value for PDI Control register (0x140 - 0x141)
const pdi_control: Field<u16> = Field::simple(WORD*0x0000);
/// Initialization value for PDI Configuration register (0x150-0x151)
const pdi_config: Field<u16> = Field::simple(WORD*0x0001);
/// Sync Impulse in multiples of 10 ns
const sync_impulse: Field<u16> = Field::simple(WORD*0x0002);
/// intialization value for PDI Configuration register R8 most significant word (0x152-0x153
const pdi_config2: Field<u16> = Field::simple(WORD*0x0003);
/// Alias Address
const address_alias: Field<u16> = Field::simple(WORD*0x0004);
/// low byte contains remainder of division of word 0 to word 6 as unsigned number divided by the polynomial x^8+x^2+x+1(initial value 0xFF)
const checksum: Field<u16> = Field::simple(WORD*0x0007);

mod device {
    use super::*;
    
    const vendor: Field<u32> = Field::simple(WORD*0x0008);
    const product: Field<u32> = Field::simple(WORD*0x000a);
    const revision: Field<u32> = Field::simple(WORD*0x000c);
    const serial_number: Field<u32> = Field::simple(WORD*0x000e);
}

mod mailbox {
    use super::*;
    
    mod boostrap {
        mod receive {
            use super::*;
    
            /// Send Mailbox Offset for Bootstrap state (slave to master)
            const offset: Field<u16> = Field::simple(WORD*0x0014);
            /// Send Mailbox Size for Bootstrap state (slave to master)
            /// Standard Mailbox size and Bootstrap Mailbox can differ. A bigger Mailbox size in Bootstrap mode can be used for optimiziation
            const size: Field<u16> = Field::simple(WORD*0x0015);
        }
        mod send {
            use super::*;
    
            /// Receive Mailbox Offset for Standard state (master to slave)
            const offset: Field<u16> = Field::simple(WORD*0x0016);
            /// Receive Mailbox Size for Standard state (master to slave)
            const size: Field<u16> = Field::simple(WORD*0x0017);
        }
    }
    mod standard {
        mod receive {
            use super::*;
    
            /// Receive Mailbox Offset for Standard state (master to slave)
            const offset: Field<u16> = Field::simple(WORD*0x0018);
            /// Receive Mailbox Size for Standard state (master to slave)
            const size: Field<u16> = Field::simple(WORD*0x0019);
        }
        mod send {
            use super::*;
    
            /// Send Mailbox Offset for Standard state (slave to master)
            const offset: Field<u16> = Field::simple(WORD*0x001a);
            /// Send Mailbox Size for Standard state (slave to master)
            const size: Field<u16> = Field::simple(WORD*0x001b);
        }
    }
    /// Mailbox Protocols Supported as defined in ETG.1000.6 Table 18
    const protocols: Field<MailboxType> = Field::simple(WORD*0x001c);
}

/**
    size of E2PROM in [KiBit] + 1
    NOTE: KiBit means 1024 Bit.
    NOTE: size = 0 means a EEPROM size of 1 KiBit
*/
const protocol: Field<u16> = Field::simple(WORD*0x003e);
/// This Version is 1
const version: Field<u16> = Field::simple(WORD*0x003f);




/// implementation of the Slave Information Interface (SII) to communicate with a slave's EEPROM memory
struct Sii<'a> {
    master: &'a RawMaster,
    slave: u16,
    unit: u16,
}
impl Sii<'_> {
    async fn new(master: &'a RawMaster, slave: u16) -> Self {
        let control = master.fprd(slave, registers::sii::control).await;
        let unit = match control.address_unit {
            Byte => 1,
            Word => 2,
        };
        Self {master, slave, unit}
    }
    /// read data from the slave's EEPROM using the SII
    async fn read<T>(&mut self, field: Field<T>) -> T {
        let buffer = T::Packed::uninit();
        let mut cursor = Cursor::new(buffer.as_mut());
        while cursor.remain().len() {
            // TODO: wait for interface to become available
            self.master.fpwr(self.slave, registers::sii::address, ((field.byte + cursor.position()) as u16)/unit).await;
            cursor.pack(&self.master.fprd(self.slave, registers::sii::data).await).unwrap();
        }
        T::unpack(buffer.as_ref())
        
        // TODO:  change the segment size using read_size
    }
    /// write data to the slave's EEPROM using the SII
    async fn write<T>(&mut self, field: Field<T>, value: &T) {
        let buffer = T::Packed::uninit();
        value.pack(buffer.as_mut());
        let mut cursor = Cursor::new(buffer.as_mut());
        while cursor.remain().len() {
            // TODO: wait for interface to become available
            self.master.fpwr(self.slave, registers::sii::address, ((field.byte + cursor.position()) as u16)/unit).await;
            self.master.fpwr(self.slave, registers::sii::data, cursor.unpack().unwrap()).await;
        }
        
        // TODO:  check possible write errors
    }
}

struct SiiIterator {}
impl Iterator for SiiIterator {
    fn next(&mut self) -> Option<CategoryItem> {todo!}
}



/// header for a SII category
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct CategoryHeader {
    /// Category Type as defined in ETG.1000.6 Table 19
    category: CategoryType,
    /// Vendor Specific
    specific: u1,
    /// Following Category Word Size x
    size: u16,
}
bilge_pdudata!(CategoryHeader, u32);

/// type of category in the SII
#[bitsize(u15)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
enum CategoryType {
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

#[repr(packed)]
struct CategoryGeneral {
    /// Group Information (Vendor specific) - Index to STRINGS
    group: u8,
    /// Image Name (Vendor specific) - Index to STRINGS
    img: u8,
    /// Device Order Number (Vendor specific) - Index to STRINGS
    order: u8,
    /// Device Name Information (Vendor specific) - Index to STRINGS
    name: u8,
    _reserved: u8,
    coe: CoeDetails,
    foe: FoeDetails,
    eoe: EoeDetails,
    _reserved: [u8;3],
    flags: GeneralFlags,
    /// EBus Current Consumption in mA, negative Values means feeding in current feed in sets the available current value to the given value
    ebus_current: i16,
    /// Index to Strings – duplicate for compatibility reasons
    group2: u8,
    _reserved: u8,
    /// Description of Physical Ports
    ports: PhysicialPorts,
    /// Element defines the ESC memory address where the Identification ID is saved if Identification Method = IdentPhyM
    identification_address: u16,
}

#[bitsize(8)]
struct CoeDetails {
    enable_sdo: bool,
    enable_sdo_info: bool,
    enable_pdo_assign: bool,
    enable_pdo_config: bool,
    enable_startup_upload: bool,
    enable_sdo_complete: bool,
    _reserved: u2,
}
#[bitsize(8)]
struct FoeDetails {
    enable: bool,
    _reserved: u7,
}
#[bitsize(8)]
struct EoeDetails {
    enable: bool,
    _reserved: u7,
}
#[bitsize(8)]
struct GeneralFlags {
    enable_safeop: bool,
    enable_notlrw: bool,
    mbox_dll: bool,
    /// ID selector mirrored in AL Statud Code
    ident_alsts: bool,
    /// ID selector value mirrored in specific physical memory as deonted by the parameter “Physical Memory Address”
    ident_phym: bool,
    _reserved: u3,
}

#[bitsize(16)]
struct PhysicialPorts {
    ports: [PhysicalPort; 4];
}
#[bitisze(4)]
enum PhysicalPort {
    Disabled = 0x0,
    Mii = 0x1,
    Reserved = 0x2,
    Ebus = 0x3,
    /// NOTE: Fast Hot Connect means a Port with Ethernet Physical Layer and Autonegotiation off (100Mbps fullduplex)
    FastHotconnect = 0x4,
}

/// ETG.1000.6 table 23
#[bitsize(8)]
enum FmmuUsage {
    #[fallback]
    Disabled = 0,
    Outputs = 1,
    Inputs = 2,
    SyncManagerStatus = 3,
}

/// ETG.1000.6 table 24
#[bitsize(64)]
struct CategorySyncManager {
    /// Origin of Data (see Physical Start Address of SyncM)
    address: u16,
    length: u16,
    /// Defines Mode of Operation (see Control Register of SyncM)
    control: u8,
    /// don't care
    status: u8,
    enable: SyncManagerEnable,
    usage: SyncManagerUsage,
}

#[bitsize(8)]
struct SyncManagerEnable {
    enable: bool,
    /// fixed content (info for config tool –SyncMan has fixed content)
    fixed_content: bool,
    /// virtual SyncManager (virtual SyncMan – no hardware resource used)
    virtual_sync_manager: bool,
    /// opOnly (SyncMan should be enabled only in OP state)
    oponly: bool,
    _reserved: u4,
}

#[bitsize(8)]
enum SyncManagerUsage {
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
