/*!
    This module provide helper structs to configure and use the memory mappings of an arbitrary bunch of slaves.
    
    Mapping of slave's physical memories to the logical memory is not mendatory but is recommended for saving bandwidth and avoid latencies in realtime operations.
    
    It highlights
    - [Mapping] to create a mapping configuration of contiguous logical memory for multiple slaves, and compute each inserted value's offsets
    - [Group] to use exchange such contiguous logical memory with an ethercat segment
    
    Example
    
        // establish mapping, it only concerns one group
        let mapping = Mapping::new()
            let slave = mapping.push(42)
                let syncmanager = slave.push(Input)
                    let pdo = syncmanager.push(Pdo{index: 0x1600, entries: 10})
                        let position = pdo.push(Sdo::<i32>::complete(0x1234))
                        let status = pdo.push(Sdo::<StatusWord>::subitem(0x1234, 1))
                        pdo.finish()
                    // possibly other PDOs
                    syncmanager.finish()
                // possibly other sync managers
            // possibly other slaves
        mapping.finish(None)
        
        // configuration of slaves
        mapping.apply(slave)
        
        // realtime exchanges
        group = Group::new(mapping)
        group.exchange()
        group.set(position, group.get(position)+velocity)
*/

use crate::{
    rawmaster::{RawMaster, PduCommand, SlaveAddress},
    data::{PduData, Field},
    sdo::{self, Sdo},
    slave::Slave,
    registers,
    };
use core::{
    ops::Range,
    cell::RefMut,
    };
use std::{
    cell::RefCell,
    collections::{HashMap, BTreeMap},
    sync::{Arc, Weak},
    };
use bilge::prelude::*;
    
pub use crate::registers::SyncDirection;








pub struct Allocator<'a> {
    master: &'a RawMaster,
    config: HashMap<u16, Weak<ConfigSlave>>,
    free: BTreeMap<usize, usize>,
}
impl<'a> Allocator<'a> {
    pub fn group(&self, mapping: Mapping<'_>) -> Group<'a> {todo!()}
}

/**
    Allows to use a contiguous slice of logical memory, with appropriate duplex buffering for read/write operations.
    
    This can typically be though to as a group of slaves, except this only manage logical memory data without any assumption on its content. It is hence unable to perform any multi-slave exception management.
*/
pub struct Group<'a> {
    master: &'a RawMaster,
    config: HashMap<u16, Arc<ConfigSlave>>,
    /// byte offset of this data group in the logical memory
    offset: u32,
    /// byte size of this data group in the logical memory
    size: u32,
    /// number of slaves in the group
    slaves: u16,
    /// data duplex: data to read from slave
    read: Vec<u8>,
    /// data duplex: data to write to slave
    write: Vec<u8>,
}
impl Group<'_> {
    pub async fn configure(&self, slave: &Slave<'_>)  {
        let master = unsafe{ slave.raw_master() };
        let address = match slave.address() {
            SlaveAddress::Fixed(a) => a,
            _ => panic!("address must be fixed before configuring a mapping"),
            };
        let config = &self.config[&address];
        
//         config.fmmu.iter().enumerate().map(|(i, entry)|  async {
//             master.fpwr(address, registers::fmmu.entry(i as u8), registers::FmmuEntry::new(
//                 self.offset + entry.logical,
//                 entry.length,
//                 0,  // null start bit:  only byte mapping is supported at the moment
//                 0,  // null end bit:  only byte mapping is supported at the moment
//                 entry.physical,
//                 0,  // null start bit
//                 false, // not used here
//                 false, // not used here
//                 true, // enable
//                 )).await.one();
//             })
//             .collect::<heapless::Vec<_, registers::fmmu.num>>()    // TODO: use Iterator::array_chunks()  as soon as stable
//             .join().await; 
        
        // FMMU mapping
        for (i, entry) in config.fmmu.iter().enumerate() {
            master.fpwr(address, registers::fmmu.entry(i as u8), {
                let mut config = registers::FmmuEntry::default();
                config.set_logical_start_byte((self.offset as u32) + entry.logical);
                config.set_logical_len_byte(entry.length);
                config.set_logical_start_bit(u3::new(0));
                config.set_logical_end_bit(u3::new(0));
                config.set_physical_start_byte(entry.physical);
                config.set_physical_start_bit(u3::new(0));
                config.set_enable(true);
                config
                }).await.one();
        }
        let mut coe = slave.coe().await;
        // PDO mapping
        for pdo in config.pdos.values() {
            for (i, sdo) in pdo.sdos.iter().enumerate() {
                // PDO mapping
                coe.sdo_write(&pdo.config.slot(i as u8), u2::new(0), sdo::PdoEntry::new(
                    sdo.index,
                    sdo.sub.unwrap(),
                    (sdo.field.len * 8).try_into().expect("field too big for a subitem"),
                    )).await;
            }
            coe.sdo_write(&pdo.config.len(), u2::new(0), pdo.sdos.len() as u8).await;
        }
        let mut offset = todo!();
        for (&i, channel) in config.channels.iter() {
            let mut size = 0;
            // sync mapping
            for (j, &pdo) in channel.pdos.iter().enumerate() {
                coe.sdo_write(&channel.config.slot(j as u8), u2::new(0), pdo).await;
                size += config.pdos[&pdo].sdos.iter()
                            .map(|sdo| sdo.field.len as u16)
                            .sum::<u16>();
            }
            coe.sdo_write(&channel.config.len(), u2::new(0), channel.pdos.len() as u8).await;
            // enable sync channel
            master.fpwr(address, registers::sync_manager::interface.mappable(i), {
                let mut config = registers::SyncManagerChannel::default();
                config.set_address(offset);
                config.set_length(size);
                config.set_mode(todo!());
                config.set_direction(channel.direction);
                config.set_enable(true);
                config
                }).await.one();
            
            offset += size;
        }
    }
    /// read and write relevant data from master to segment
    pub async fn exchange(&mut self) -> &'_ mut [u8]  {
        // TODO: offset should be passed as 32 bit address, this requires a modification of RawMaster
        assert_eq!(self.master.pdu(PduCommand::LRW, 0, self.offset as u16, self.write.as_mut_slice()).await, self.slaves);
        self.write.as_mut_slice()
    }
    /// read data slice from segment
    pub async fn read(&mut self) -> &'_ mut [u8]  {
        assert_eq!(self.master.pdu(PduCommand::LRD, 0, self.offset as u16, self.read.as_mut_slice()).await, self.slaves);
        self.read.as_mut_slice()
    }
    /// write data slice to segment
    pub async fn write(&mut self) -> &'_ mut [u8]  {
        assert_eq!(self.master.pdu(PduCommand::LWR, 0, self.offset as u16, self.write.as_mut_slice()).await, self.slaves);
        self.write.as_mut_slice()
    }
    
    /// extract a mapped value from the buffer of last received data
    pub fn get<T: PduData>(&mut self, field: Field<T>) -> T  {field.get(&self.read)}
    /// pack a mapped value to the buffer for next data write
    pub fn set<T: PduData>(&mut self, field: Field<T>, value: T)  {field.set(&mut self.write, value)}
}


/// struct holding configuration informations for multiple slaves, that can be shared between multiple mappings
pub struct Config {
    slaves: RefCell<HashMap<u16, RefCell<ConfigSlave>>>,
}
/// configuration for one slave
struct ConfigSlave {
    pdos: HashMap<u16, ConfigPdo>,
    channels: HashMap<u8, ConfigChannel>,
    fmmu: Vec<ConfigFmmu>,
}
struct ConfigPdo {
    config: sdo::Pdo,
    sdos: Vec<Sdo>,
}
/// configuration for a slave sync manager channel
struct ConfigChannel {
    direction: SyncDirection,
    config: sdo::SyncChannel,
    pdos: Vec<u16>,
}
/// configuration for a slave FMMU
struct ConfigFmmu {
    length: u16,
    physical: u16,
    logical: u32,
}


/**
    Convenient struct to create a memory mapping for multiple slaves to the logical memory (responsible for realtime data exchanges).
    
    It is always slave's physical memory that is mapped to the logical memory. [crate::registers] is giving the set of standard values in the physical memory. Any other values must be configured to be present in the physical memory, such as described in [crate::can::Can].
    
    This struct (and fellows) provide ways to map every possible thing to the logical memory. Each value-insertion method is returning a [Field] pointing to the position of the mapped value in the contiguous slice configured here (its offset is relative to the slice start and not to the logical memory start).
    
    The pushed values will be mapped in the exact order they will be pushed. Depending on the memory layout desired, push calls must be ordered accordingly.
    
    The FMMU (Fieldbux Memory Mapping Unit) is hidden from the user and is used to adjust variables order.
*/
pub struct Mapping<'a> {
    config: &'a Config,
    offset: RefCell<usize>,
}
impl<'a> Mapping<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config, offset: RefCell::new(0) }
    }
    /// create an object for mapping data from a given slave
    ///
    /// data coming for different slaves can interlace, hence multiple slave mapping instances can exist at the same time
    pub fn slave(&self, address: u16) -> MappingSlave<'_>  {
        self.config.slaves.borrow_mut()
            .entry(address)
            .or_insert_with(|| RefCell::new(ConfigSlave {
                pdos: HashMap::new(),
                channels: HashMap::new(),
                fmmu: Vec::new(),
                }));
        MappingSlave {
            mapping: self,
            config: self.config.slaves.borrow().get(&address).unwrap().borrow_mut(),
        }
    }
}
/// object allowing to map data from a slave
///
/// data coming from one's slave physical memory shall not interlace (this is a limitation due to this library, not ethercat) so any mapping methods in here are preventing multiple mapping instances
pub struct MappingSlave<'a> {
    mapping: &'a Mapping<'a>,
    config: RefMut<'a, ConfigSlave>,
}
impl MappingSlave<'_> {
    /// internal method to increment the logical and physical offsets with the given data length
    /// if the logical offset was changed since the last call to this slave's method (ie. the logical memory contiguity is broken), a new FMMU is automatically configured
    fn insert(&mut self, length: usize) -> usize {
        let mut offset = self.mapping.offset.borrow_mut();
        let current = self.config.fmmu.last().unwrap();
        if (current.logical + current.length as u32) != *offset as u32 {
            let new = ConfigFmmu {
                length: 0, 
                logical: *offset as u32, 
                physical: current.physical,
                };
            self.config.fmmu.push(new);
        }
        let current = self.config.fmmu.last_mut().unwrap();
        current.length += length as u16;
        *offset += length;
        offset.clone()
    }
    /// map a range of physical memory, and return its matching range in the logical memory
    pub fn range(&mut self, range: Range<u16>) -> Range<usize> {
        let size = usize::from(range.end - range.start);
        let start = self.insert(size);
        Range {start, end: start+size}
    }
    /// map a field in the physical memory (a register), and return its matching field in the logical memory
    pub fn register<T: PduData>(&mut self, field: Field<T>) -> Field<T>  {
        Field::new(self.insert(field.len), field.len)
    }
    /// map a sync manager channel
    pub fn channel(&mut self, index: u8, direction: SyncDirection, sdo: sdo::SyncChannel) -> MappingChannel<'_> {
        self.config.channels.insert(index, ConfigChannel {
            direction,
            config: sdo,
            pdos: Vec::new(),
            });
        let entries = &mut self.config.channels.get(&index).unwrap().pdos;
        MappingChannel {
            slave: self,
            entries: unsafe {&mut *(entries as *const _ as *mut _)},
        }
    }
}
pub struct MappingChannel<'a> {
    slave: &'a mut MappingSlave<'a>,
    entries: &'a mut Vec<u16>,
}
impl MappingChannel<'_> {
    /// add a pdo to this channel, and return an object to map it
    pub fn push(&mut self, pdo: sdo::Pdo) -> MappingPdo<'_>  {
        self.entries.push(pdo.index);
        self.slave.config.pdos.insert(pdo.index, ConfigPdo {
            config: pdo,
            sdos: Vec::new(),
            });
        let entries = &self.slave.config.pdos.get(&pdo.index).unwrap().sdos;
        MappingPdo {
            slave: self.slave,
            entries: unsafe {&mut *(entries as *const _ as *mut _)},
        }
    }
}
pub struct MappingPdo<'a> {
    slave: &'a mut MappingSlave<'a>,
    entries: &'a mut Vec<Sdo>,
}
impl MappingPdo<'_> {
    /// add an sdo to this channel, and return its matching field in the logical memory
    pub fn push<T: PduData>(&mut self, sdo: Sdo<T>) -> Field<T> {
        self.entries.push(sdo.clone().downcast());
        Field::new(self.slave.insert(sdo.field.len), sdo.field.len)
    }
}



// let group = mapping.group()
// group.slave(0).add_pdo(Pdo {...}, [Sdo::sub(...)])
// group.slave(0).add_channel(direction, [u16])
// 
// group.slave(0).pdo(Pdo {...}).push(Sdo::sub(...)) -> Field
// group.slave(0).pdo(Pdo {...}).push(Sdo::sub(...)) -> Field
// group.slave(0).sdo(input, Sdo::sub(...)) -> Field
// group.slave(0).sdo(input, Sdo::sub(...)) -> Field
// group.slave(0).sdo(output, Sdo::sub(...)) -> Field
// group.slave(0).register(Field::simple(...)) -> Field
// group.slave(0).range(Range<u16>) -> Range<usize>

