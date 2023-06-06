/*! 
Convenient structures to read/write the slave's dictionnary objects (SDO) and configure mappings.

# Example of PDO mapping

	// typical mapping
	// the SDOs are declared first somewhere
	let rx = SyncManager {index: 0x1c12, num: 3};
	let pdo = ConfigurablePdo {index: 0x1600, num: 3};
	let target_controlword = SubItem::<ControlWord> {index: 0x6040, sub: 0, field: Field::new(0, 2)};
	let target_position = SubItem::<i32> {index: 0x607a, sub: 0, field: Field::new(0, 4)};
	
	// the mapping is done at program start
	let mut sm = SyncMapping::new(slave, &rx).await?;
	let mut pdo = sm.push(&pdo).await?;
	let offset_controlword = pdo.push(&target_controlword).await?;
	let offset_position = pdo.push(&target_position).await?;
	pdo.finish().await?;
	sm.finish().await?;
	
	// typical use latter in the program
	offset_controlword.set(slave.outputs(), ...);
*/

use crate::{
// 	slave::Slave,
	data::{BitField, PduData, Storage},
	};
use core::fmt;


/// description of an SDO's subitem, not a SDO itself
#[derive(Clone)]
pub struct Sdo<T: PduData> {
	/// index of the item in the slave's dictionnary of objects
	pub index: u16,
	/// subindex in the item
	pub sub: SdoPart,
	/// field pointing to the subitem in the byte sequence of the complete SDO
	pub field: BitField<T>,
}
/// specifies which par of an SDO is addressed
#[derive(Copy, Clone, Debug)]
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
#[derive(Clone)]
pub struct ConfigurablePdo {
	/// index of the SDO that configures the PDO
	pub index: u16,
	/// number of entries in the PDO
	pub num: u8,
}
impl fmt::Debug for ConfigurablePdo {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ConfigurablePdo {{index: {:x}, num: {}}}", self.index, self.num)
	}
}

/// description of SDO configuring a SyncManager
/// the SDO is assumed to follow the cia402 specifications for syncmanager SDOs
#[derive(Clone)]
pub struct SyncManager {
	/// index of the SDO that configures the SyncManager
	pub index: u16,
	/// max number of PDO that can be assigned to the SyncManager
	pub num: u8,
}
impl fmt::Debug for SyncManager {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ConfigurablePdo {{index: {:x}, num: {}}}", self.index, self.num)
	}
}
