/*! 
Convenient structures to read/write the slave's dictionnary objects (SDO) and configure mappings...);
*/

use crate::{
// 	slave::Slave,
	data::{self, BitField, PduData, Storage},
	};
use core::fmt;
use bilge::prelude::*;


/// address of an SDO's subitem, not a SDO itself
#[derive(Eq, PartialEq)]
pub struct Sdo<T: PduData=()> {
	/// index of the item in the slave's dictionnary of objects
	pub index: u16,
	/// subindex in the item
	pub sub: SdoPart,
	/// field pointing to the subitem in the byte sequence of the complete SDO
	pub field: BitField<T>,
}
/// specifies which par of an SDO is addressed
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum SdoPart {
    /// the whole SDO (the complete struct with its eventual paddings)
    /// NOTE: this doesn't strictly follows the ethercat specifications, since for complete SDO request we could choose to include or exclude subitem 0
    Complete,
    /// one subitem value in the SDO
    Sub(u8),
}
impl<T: PduData> Sdo<T> {
	/// address an sdo subitem, deducing its bit size from the `PduData` impl
	/// offset is the bit offset of the subitem in the complete sdo
	pub const fn sub(index: u16, sub: u8, offset: usize) -> Self { Self{
		index,
		sub: SdoPart::Sub(sub),
		field: BitField::new(offset, T::Packed::LEN*8),
	}}
	pub const fn sub_with_size(index: u16, sub: u8, offset: usize, size: usize) -> Self { Self{
		index,
		sub: SdoPart::Sub(sub),
		field: BitField::new(offset, size),
	}}
	/// address a complete sdo at the given index, with `sub=0` and `byte=0`
	pub const fn complete(index: u16) -> Self { Self{ 
		index, 
		sub: SdoPart::Complete, 
		field: BitField::new(0, T::Packed::LEN*8),
	}}
	pub const fn complete_with_size(index: u16, size: usize) -> Self { Self{ 
		index, 
		sub: SdoPart::Complete, 
		field: BitField::new(0, size),
	}}
	
	pub fn downcast(self) -> Sdo { Sdo{
        index: self.index,
        sub: self.sub,
        field: BitField::new(self.field.bit, self.field.len),
	}}
	
// 	/// retreive the current subitem value from the given slave
// 	pub async fn get(&self, slave: &Slave) -> T  {todo!()}
// 	/// set the subitem value on the given slave
// 	pub async fn set(&self, slave: &Slave, value: T)   {todo!()}
}
impl SdoPart {
    /// return the subindex or 0 for a complete item
    pub fn unwrap(self) -> u8 { match self {
            Self::Complete => 0,
            Self::Sub(i) => i,  
    }}
    /// true if this SDO sdo address refers to a complete SDO, false if it refers to a subitem
    pub fn is_complete(&self) -> bool { match self {
            Self::Complete => true,
            _ => false,
    }}
}
impl<T: PduData> fmt::Debug for Sdo<T> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "Sdo {{index: 0x{:x}, sub: {:?}, field: {:?}}}", self.index, self.sub, self.field)
	}
}
// [Clone] and [Copy] must be implemented manually to allow copying a sdo pointing to a type which does not implement this operation
impl<T: PduData> Clone for Sdo<T> {
    fn clone(&self) -> Self { Self {
        index: self.index,
        sub: self.sub,
        field: BitField::new(self.field.bit, self.field.len)
    }}
}
impl<T: PduData> Copy for Sdo<T> {}

/// description of SDO configuring a PDO
/// the SDO is assumed to follow the cia402 specifications for PDO SDOs
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Pdo {
	/// index of the SDO that configures the PDO
	pub index: u16,
	/// number of entries in the PDO
	pub num: u8,
}
impl Pdo {
    /// return a field pointing to the nth entry definition of the PDO
    pub fn slot(&self, i: u8) -> Sdo<PdoEntry> {
        Sdo::sub(self.index, i+1, core::mem::size_of::<u8>() + core::mem::size_of::<PdoEntry>()*usize::from(i))
    }
    /// return a field pointing to the number of items set in the PDO
    pub const fn len(&self) -> Sdo<u8> {
        Sdo::sub(self.index, 0, 0)
    }
}
impl fmt::Debug for Pdo {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "Pdo {{index: 0x{:x}, num: {}}}", self.index, self.num)
	}
}

/// content of a subitem in an SDO for PDO mapping
#[bitsize(32)]
pub struct PdoEntry {
    /// mapped sdo index
    index: u16,
    /// mapped sdo subindex (it is not possible to map complete sdo, so this field must always be set)
    sub: u8,
    /// bit size of the subitem value
    bitsize: u8,
}
data::bilge_pdudata!(PdoEntry, u32);

/// ETG.1000.6 table 67
pub struct SyncManager {
    /// index of first SDO configuring a [SyncChannel]
    pub index: u16,
    /// number of SDOs configuring SyncChannels
    pub num: u8,
}
impl SyncManager {
    pub fn channel(&self, i: u8) -> SyncChannel {
        SyncChannel { index: self.index + u16::from(i), num: 254 }
    }
}

/**
    description of SDO configuring a SyncChannel
    
    the SDO is assumed to follow the cia402 specifications for syncmanager SDOs 
    (ETG.1000.6 table 77)
*/
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct SyncChannel {
	/// index of the SDO that configures the SyncChannel
	pub index: u16,
	/// max number of PDO that can be assigned to the SyncChannel
	pub num: u8,
}
impl SyncChannel {
    /// return a field pointing to the nth entry definition of the sync manager channel
    pub fn slot(&self, i: u8) -> Sdo<u16> {
        Sdo::sub(self.index, i+1, core::mem::size_of::<u8>() + core::mem::size_of::<u16>()*usize::from(i))
    }
    /// return a field pointing to the number of items set in the sync manager channel
    pub const fn len(&self) -> Sdo<u8> {
        Sdo::sub(self.index, 0, 0)
    }
}
impl fmt::Debug for SyncChannel {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "SyncChannel {{index: 0x{:x}, num: {}}}", self.index, self.num)
	}
}

const device_type: Sdo<u32> = Sdo::complete(0x0000);
const error: Sdo<u8> = Sdo::complete(0x0001);
// const device_name: Sdo<str> = Sdo::complete(0x0008);
// const hardware_version: Sdo<str> = Sdo::complete(0x0009);
// const software_version: Sdo<str> = Sdo::complete(0x000a);
// const identity: Sdo<record> = Sdo::complete(0x0018);
// const receive_pdos: PdoMappings = PdoMappings {index: 0x1600; num: 512};
// const transmit_pdos: PdoMappings = PdoMappings {index: 0x1a00; num: 512};
// const sync_manager: SyncManager = SyncManager {index: 0x1c10, num: 254};
