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
    rawmaster::{RawMaster, PduCommand},
    data::{PduData, Field},
    sdo::{self, Sdo, Pdo, SyncManager},
    slave::Slave,
    registers,
    };
use core::ops::Range;
use std::{
    cell::RefCell,
    collections::HashMap,
    };
    
pub use crate::registers::SyncDirection;

/**
    Allows to use a contiguous slice of logical memory, with appropriate duplex buffering for read/write operations.
    
    This can typically be though to as a group of slaves, except this only manage logical memory data without any assumption on its content. It is hence unable to perform any multi-slave exception management.
*/
pub struct Group<'a> {
    master: &'a RawMaster,
    /// byte offset of this data group in the logical memory
    offset: u16,
    /// byte size of this data group in the logical memory
    size: u16,
    /// number of slaves in the group
    slaves: u16,
    /// data duplex: data to read from slave
    read: Vec<u8>,
    /// data duplex: data to write to slave
    write: Vec<u8>,
}
impl Group<'_> {
    /// read and write relevant data from master to segment
    pub async fn exchange(&mut self) -> &'_ mut [u8]  {
        assert_eq!(self.master.pdu(PduCommand::LRW, 0, self.offset, &mut self.write), self.slaves);
        &mut self.write
    }
    /// read data slice from segment
    pub async fn read(&mut self) -> &'_ mut [u8]  {
        assert_eq!(self.master.pdu(PduCommand::LRD, 0, self.offset, &mut self.read), self.slaves);
        &mut self.read
    }
    /// write data slice to segment
    pub async fn write(&mut self) -> &'_ mut [u8]  {
        assert_eq!(self.master.pdu(PduCommand::LWR, 0, self.offset, &self.write), self.slaves);
        &mut self.write
    }
    
    /// extract a mapped value from the buffer of last received data
    pub fn get<T: PduData>(&mut self, field: Field<T>) -> T  {field.get(&self.read)}
    /// pack a mapped value to the buffer for next data write
    pub fn set<T: PduData>(&mut self, field: Field<T>, value: T)  {field.set(&mut self.write, value)}
}


/**
    Convenient struct to create a memory mapping for multiple slaves to the logical memory (responsible for realtime data exchanges).
    
    It is always slave's physical memory that is mapped to the logical memory. [crate::registers] are giving the set of standard values in the physical memory. Any other values must be configured to be present in the physical memory, such as described in [crate::can::Can].
    
    This struct (and fellows) provide ways to map every possible thing to the logical memory. Each value-insertion method is returning a [Field] pointing to the position of the mapped value in the contiguous slice configured here (its offset is relative to the slice start and not to the logical memory start).
    
    The pushed values will be mapped in the exact order they will be pushed. Depending on the memory layout desired, push calls must be ordered accordingly.
    
    The FMMU (Fieldbux Memory Mapping Unit) is hidden from the user and is used to adjust variables order.
*/
pub struct Mapping {
    /// configuration for each slave in this mapping
    slaves: RefCell<HashMap<u16, Vec<Channel>>>,
    /// current sdo cursor in the memory of the group
    cursor: RefCell<usize>,
    /// global offset of the memory group in the logical memory
    offset: usize,
}
struct Channel {
    range: Range<usize>,
    sdos: Vec<Vec<Vec<Sdo>>>,
}

impl Mapping {
    /// configure the given slave with the current mapping, so the slave will provide the values pushed to the mapping at the memory position requested by the mapping.
    pub async fn apply(&self, slave: &mut Slave<'_>) {
        let address = slave.address();
        let master = slave.master;
        let config = self.slaves[&address];
        
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
            master.fpwr(address, registers::fmmu.entry(i as u8), registers::FmmuEntry::new(
                self.offset + entry.logical,
                entry.length,
                0,  // null start bit:  only byte mapping is supported at the moment
                0,  // null end bit:  only byte mapping is supported at the moment
                entry.physical,
                0,  // null start bit
                false, // not used here
                false, // not used here
                true, // enable
                )).await.one();
        }
        let coe = slave.coe();
        for (i, channel) in config.channels.iter().enumerate() {
            for (i, pdo) in channel.pdos.iter().enumerate() {
                for sdo in pdo.sdos.iter().enumerate() {
                    // PDO mapping
                    coe.write_sdo(pdo.config.slot(i as u8), sdo::PdoEntry::new(
                        sdo.index,
                        sdo.sub.unwrap(),
                        sdo.field.len * 8,
                        )).await;
                }
                coe.write_sdo(pdo.config.len(), pdo.sdos.len() as u8).await;
                // Sync mapping
                coe.write_sdo(channel.config.slot(i as u8), pdo.config.index).await;
            }
            coe.write_sdo(channel.config.len(), channel.pdos.len() as u8).await;
            // enable sync channel
            master.fpwr(address, registers::sync_manager::interface.mappable(i as u8), {
                let config = registers::SyncManagerChannel::default();
                config.set_address(channel.offset);
                config.set_length(channel.size);
                config.set_mode(todo!());
                config.set_direction(channel.direction);
                config.set_enable(true);
                config
                }).await.one();
        }
    }
    
    pub fn slave(&self, address: u16) -> MappingSlave<'_> {
        MappingSlave {
            address,
            channels: RefCell::new(self.slaves.borrow_mut()
                                    .remove(address)
                                    .or_default()),
            cursor: &self.cursor,
        }
    }
    pub fn finish(self) {
        todo!()
    }
}

pub struct MappingSlave<'a> {
    address: u16,
    channels: RefCell<Vec<Channel>>,
    cursor: &'a RefCell<usize>,
}

impl MappingSlave<'_> {
    pub fn register<T: PduData>(&mut self, field: Field<T>) -> Field<T> {
        todo!()
    }
    pub fn channel(&mut self, direction: SyncDirection) -> MappingChannel<'_>   {
        MappingChannel {
            pdos: Vec::new(),
            cursor: self.cursor,
        }
    }
    pub fn finish(self) {
        self.mapping.slaves.insert(self.address, self.channels.into());
    }
}

pub struct MappingChannel<'a> {
    pdos: Vec<Pdo>,
    cursor: &'a mut usize,
}

impl MappingChannel<'_> {
    pub fn push(&mut self, pdo: &Pdo) -> MappingPdo<'_> {
        MappingPdo {
            pdo,
            sdos: Vec::new(),
            cursor: self.cursor,
        }
    }
}

pub struct MappingPdo<'a> {
    pdo: &'a Pdo,
    sdos: Vec<Sdo>,
    cursor: &'a mut usize,
}

impl MappingPdo<'_> {
    pub fn push<T: PduData>(&mut self, sdo: &Sdo<T>) -> Field<T> {
        assert!(! sdo.is_complete());
        assert!(self.sdos.len() < self.pdo.num);
        
        self.sdos.push((sdo.index, sdo.sub));
        let field = Field::new(*self.cursor, sdo.field.len);
        *self.cursor += sdo.field.len;
        field
    }
}
