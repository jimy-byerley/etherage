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
#[derive(Clone, Eq, PartialEq)]
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
	pub fn sub(index: u16, sub: u8, offset: usize) -> Self { Self{
		index,
		sub: SdoPart::Sub(sub),
		field: BitField::new(offset, T::Packed::LEN*8),
	}}
	pub fn sub_with_size(index: u16, sub: u8, offset: usize, size: usize) -> Self { Self{
		index,
		sub: SdoPart::Sub(sub),
		field: BitField::new(offset, size),
	}}
	/// address a complete sdo at the given index, with `sub=0` and `byte=0`
	pub fn complete(index: u16) -> Self { Self{ 
		index, 
		sub: SdoPart::Complete, 
		field: BitField::new(0, T::Packed::LEN*8),
	}}
	pub fn complete_with_size(index: u16, size: usize) -> Self { Self{ 
		index, 
		sub: SdoPart::Complete, 
		field: BitField::new(0, size),
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
		write!(f, "Sdo {{index: {:x}, sub: {:?}, field: {:?}}}", self.index, self.sub, self.field)
	}
}

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
        Sdo::sub(self.index, i+1, core::mem::size_of::<u8>() + core::mem::size_of::<PdoEntry>()*i)
    }
    /// return a field pointing to the number of items set in the PDO
    pub fn len(&self) -> Sdo<u8> {
        Sdo::sub(self.index, 0, 0)
    }
}
impl fmt::Debug for Pdo {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "Pdo {{index: {:x}, num: {}}}", self.index, self.num)
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

/// description of SDO configuring a SyncManager
/// the SDO is assumed to follow the cia402 specifications for syncmanager SDOs
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct SyncManager {
	/// index of the SDO that configures the SyncManager
	pub index: u16,
	/// max number of PDO that can be assigned to the SyncManager
	pub num: u8,
}
impl SyncManager {
    /// return a field pointing to the nth entry definition of the sync manager channel
    pub fn slot(&self, i: u8) -> Sdo<u16> {
        Sdo::sub(self.index, i+1, core::mem::size_of::<u8>() + core::mem::size_of::<u16>()*i)
    }
    /// return a field pointing to the number of items set in the sync manager channel
    pub fn len(&self) -> Sdo<u8> {
        Sdo::sub(self.index, 0, 0)
    }
}
impl fmt::Debug for SyncManager {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "SyncManager {{index: {:x}, num: {}}}", self.index, self.num)
	}
}
