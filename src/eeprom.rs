/*!
    This module expose the standard EEPROM registers. registers are defined as [Field]s in the EEPROM, which content can be accessed using the instance of [Sii] (Slave Information Interface) proper to each slave.

    ETG.1000.6 5.4
*/

use crate::data::{self, PduData, Field};
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
