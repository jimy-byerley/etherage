/*!
    This module expose the standard slaves EEPROM registers.

    registers are defined as [Field]s in the EEPROM, which content can be accessed using the instance of [Sii][crate::sii::Sii] (Slave Information Interface) proper to each slave.

    ETG.1000.6 5.4
*/

use crate::data::{self, PduData, Field};
use crate::registers::MailboxSupport;
use bilge::prelude::*;


pub const WORD: usize = core::mem::size_of::<u16>();


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

/// standard informations about the product the slave is
pub mod device {
    use super::*;

    /// unique id of the vendor (normalized by ETG)
    pub const vendor: Field<u32> = Field::simple(WORD*0x0008);
    /// unique id of the product (normalized by the vendor)
    pub const product: Field<u32> = Field::simple(WORD*0x000a);
    /// unique id of the product revision (normalized by the vendor)
    pub const revision: Field<u32> = Field::simple(WORD*0x000c);
    /// unique serial number of the product (normalized by the vendor)
    pub const serial_number: Field<u32> = Field::simple(WORD*0x000e);
}

/// recommended configuration for the mailbox
pub mod mailbox {
    use super::*;

    /// mailbox recommended parameters during [Bootstrap](crate::CommunicationState::Bootstrap) state
    pub mod bootstrap {
        use super::*;
        pub mod write {
            use super::*;

            /// Send Mailbox Offset for Bootstrap state (slave to master)
            pub const offset: Field<u16> = Field::simple(WORD*0x0014);
            /// Send Mailbox Size for Bootstrap state (slave to master)
            /// Standard Mailbox size and Bootstrap Mailbox can differ. A bigger Mailbox size in Bootstrap mode can be used for optimiziation
            pub const size: Field<u16> = Field::simple(WORD*0x0015);
        }
       pub  mod read {
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
        pub mod write {
            use super::*;

            /// Receive Mailbox Offset for Standard state (master to slave)
            pub const offset: Field<u16> = Field::simple(WORD*0x0018);
            /// Receive Mailbox Size for Standard state (master to slave)
            pub const size: Field<u16> = Field::simple(WORD*0x0019);
        }
        pub mod read {
            use super::*;

            /// Send Mailbox Offset for Standard state (slave to master)
            pub const offset: Field<u16> = Field::simple(WORD*0x001a);
            /// Send Mailbox Size for Standard state (slave to master)
            pub const size: Field<u16> = Field::simple(WORD*0x001b);
        }
    }
    /// Mailbox Protocols Supported as defined in ETG.1000.6 Table 18
    pub const protocols: Field<MailboxSupport> = Field::simple(WORD*0x001c);
}

/**
    size of EEPROM in KiBit + 1

    NOTE: KiBit means 1024 Bit.

    NOTE: size = 0 means a EEPROM size of 1 KiBit
*/
pub const eeprom_size: Field<u16> = Field::simple(WORD*0x003e);
/// This Version is 1
pub const version: Field<u16> = Field::simple(WORD*0x003f);
/// start addresse for categories
pub const categories: u16 = (WORD*0x0040) as u16;
