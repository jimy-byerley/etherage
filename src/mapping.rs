/*!
    This module provide helper structs to configure and use the memory mappings of an arbitrary bunch of slaves.

    Mapping of slave's physical memories to the logical memory is not mendatory but is recommended for saving bandwidth and avoid latencies in realtime operations. The only other way to perform realtime operations is to directly read/write the slave's physical memories.

    ## Highlights
    - [Mapping] to create a mapping configuration of contiguous logical memory for multiple slaves, and compute each inserted value's offsets
    - [Group] to use exchange such contiguous logical memory with an ethercat segment

    ## Example

    ```ignore
    // establish mapping
    let config = Config::default();
    let mapping = Mapping::new(&config);
        let slave = mapping.slave(42);
            let mut channel = slave.channel(sdo::SyncChannel{ index: 0x1c12, direction: SyncDirection::Read, num: 4 });
                let mut pdo = channel.push(sdo::Pdo{ index: 0x1600, num: 10 });
                    let status = pdo.push(Sdo::<u16>::complete(0x6041));
                    let position = pdo.push(Sdo::<i32>::complete(0x6064));
                // possibly other PDOs
            // possibly other sync managers
        // possibly other slaves
    let group = allocator.group(mapping);

    // configuration of slaves
    group.configure(slave).await;

    // realtime exchanges
    group.exchange().await;
    group.get(position);
    ```

    ## Principle

    The following scheme shows an example mapping of [SDOs](sdo) and [registers]. On the right side shows the range of PDOs and channels that can be mapped each slave, however the vendor-specific constraints makes them much smaller in practice.
    
    ![mapping details](https://raw.githubusercontent.com/jimy-byerley/etherage/master/schemes/mapping-details.svg)
    
    ## Limitations

    - mapped regions in the logical memory are forced to be in the same order as in the physical memory.

        This not due to the ethercat specifications, but is needed here to compute the mapped fields offsets on field insertion.

        Interlacing different slave's memory is however possible.

    - different instances of [Mapping] cannot request the allocator different configurations for one slave, even if they could be merged into one in the absolute.

        different mapping has to use the exact same config for one slave in order to share it. This should be acheived using the same instance of [Config]

    - the mapping is currently byte aligned, the ethercat specs allows a bit aligned mapping but this is not (yet) implemented.

    - the memory area reserved for mapping is currently limited to 768 bytes
*/

use crate::{
    rawmaster::{RawMaster, PduCommand, SlaveAddress},
    data::{PduData, Field},
    sdo::{self, Sdo, SyncDirection},
    slave::{Slave, CommunicationState},
    can::CanError,
    registers,
    error::EthercatResult,
    };
use core::{
    fmt,
    ops::{Range, Deref},
    cell::RefCell,
    };
use std::{
    collections::{HashMap, HashSet, BTreeSet},
    sync::{Arc, Weak, Mutex, RwLock, RwLockWriteGuard},
    };
use bilge::prelude::*;


/// slave physical memory range used for sync channels mapping
const SLAVE_PHYSICAL_MAPPABLE: Range<u16> = Range {start: 0x1000, end: 0x1300};


/// convenient object to manage slaves configurations and logical memory
pub struct Allocator {
    internal: Mutex<AllocatorInternal>,
}
pub struct AllocatorInternal {
    slaves: HashMap<u16, Weak<ConfigSlave>>,
    free: BTreeSet<LogicalSlot>,
}
/// allocator slot in logical memory
#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
struct LogicalSlot {
    size: u32,
    position: u32,
}
impl Allocator {
    pub fn new() -> Self {
        let mut free = BTreeSet::new();
        free.insert(LogicalSlot {size: u32::MAX, position: 0});
        let internal = Mutex::new(AllocatorInternal {
            slaves: HashMap::new(),
            free,
        });
        Self {internal}
    }
    /// allocate the memory area and the slaves for the given mapping.
    /// returning a [Group] for using that memory and communicate with the slaves
    pub fn group<'a>(&'a self, master: &'a RawMaster, mapping: &Mapping) -> Group<'a> {
        // compute mapping size
        let size = mapping.offset.borrow().clone();

        let mut internal = self.internal.lock().unwrap();
        // check that new mapping has not conflict with current config
        assert!(internal.compatible(&mapping));
        // reserve memory
        let slot;
        {
            slot = internal.free.range(LogicalSlot {size, position: 0} ..)
                        .next().expect("no more logical memory")
                        .clone();
            internal.free.remove(&slot);
            if slot.size > size {
                internal.free.insert(LogicalSlot {
                    position: slot.position + size,
                    size: slot.size - size,
                });
            }
        }
        // update global config
        let mut slaves = HashMap::<u16, Arc<ConfigSlave>>::new();
        let config = mapping.config.slaves.lock().unwrap();
        for &k in mapping.slaves.borrow().iter() {
            slaves.insert(k, 
                if let Some(value) = internal.slaves.get(&k).map(|v|  v.upgrade()).flatten() 
                    // if config for slave already existing, we can use it, because we already checked it was perfectly the same in `self.compatible()` 
                    {value}
                else {
                    let new = Arc::new(config[&k].try_read().expect("a slave is still in mapping").clone());
                    internal.slaves.insert(k, Arc::downgrade(&new));
                    new
                });
        }
        // create initial buffers
        let mut buffer = mapping.default.borrow().clone();
        buffer.extend((buffer.len() .. size as usize).map(|_| 0));
        // create group
        Group {
            allocator: self,
            allocated: slot.size,
            offset: slot.position,
            size,

            config: slaves,
            data: tokio::sync::Mutex::new(GroupData {
                master,
                offset: slot.position,
                read: buffer.clone(),
                write: buffer,
            }),
        }
    }
    /// check that a given mapping is compatible with the already configured slaves in the allocator
    /// if true, a group can be initialized from the mapping
    pub fn compatible(&self, mapping: &Mapping) -> bool {
        self.internal.lock().unwrap().compatible(mapping)
    }
    /// return the amount of allocated memory in the logical memory
    pub fn allocated(&self) -> u32 {
        self.internal.lock().unwrap().allocated()
    }
    /// return the amount of free (potentially fragmented) memory in the logical memory
    pub fn free(&self) -> u32 {
        self.internal.lock().unwrap().free()
    }
}
impl AllocatorInternal {
    /// check that a given mapping is compatible with the already configured slaves in the allocator
    /// if true, a group can be initialized from the mapping
    fn compatible(&self, mapping: &Mapping) -> bool {
        for (address, slave) in mapping.config.slaves.lock().unwrap().iter() {
            if let Some(alter) = self.slaves.get(address) {
                if let Some(alter) = alter.upgrade() {
                    if slave.try_read().expect("a slave is still in mapping").deref() != alter.as_ref()
                        {return false}
                }
            }
        }
        true
    }
    /// return the amount of allocated memory in the logical memory
    fn allocated(&self) -> u32 {
        u32::MAX - self.free()
    }
    /// return the amount of free (potentially fragmented) memory in the logical memory
    fn free(&self) -> u32 {
        self.free.iter()
            .map(|s|  s.size)
            .sum::<u32>()
    }
}
impl fmt::Debug for Allocator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let internal = self.internal.lock().unwrap();
        write!(f, "<Allocator with {} slaves using {} bytes>",
            internal.slaves.len(),
            internal.allocated(),
            )
    }
}


/**
    Allows to use a contiguous slice of logical memory, with appropriate duplex buffering for read/write operations.

    This can typically be though to as a group of slaves, except this only manage logical memory data without any assumption on its content. It is hence unable to perform any multi-slave exception management.
*/
pub struct Group<'a> {
    allocator: &'a Allocator,
    /// byte size of the allocated region of the logical memory
    allocated: u32,
    /// byte offset of this data group in the logical memory
    offset: u32,
    /// byte size of this data group in the logical memory
    size: u32,

    /// configuration per slave
    config: HashMap<u16, Arc<ConfigSlave>>,
    data: tokio::sync::Mutex<GroupData<'a>>,
}
pub struct GroupData<'a> {
    master: &'a RawMaster,
    /// byte offset of this data group in the logical memory
    offset: u32,
    /// data duplex: data to read from slave
    read: Vec<u8>,
    /// data duplex: data to write to slave
    write: Vec<u8>,
}
impl<'a> Group<'a> {
    /// the configuration of slaves
    pub fn config(&self) -> &HashMap<u16, Arc<ConfigSlave>>  {&self.config}
    /// `true` if this group has a configuration for the given slave
    pub fn contains(&self, slave: u16) -> bool {
        self.config.contains_key(&slave)
    }
    /**
        write on the given slave the matching configuration from the mapping

        the slave is assumed to be in state [PreOperational](CommunicationState::PreOperational), and can be switched to [SafeOperational](crate::CommunicationState::SafeOperational) after this step.
    */
    pub async fn configure(&self, slave: &Slave<'_>) -> EthercatResult<(), CanError> {
        let master = unsafe{ slave.raw_master() };
        let address = match slave.address() {
            SlaveAddress::Fixed(a) => a,
            _ => panic!("address must be fixed before configuring a mapping"),
            };
        let config = &self.config[&address];

        assert_eq!(slave.expected(), CommunicationState::PreOperational, "slave must be in preop state to configure a mapping");

        // range of physical memory to be mapped
        let physical = SLAVE_PHYSICAL_MAPPABLE;

        let mut coe = slave.coe().await;
        let priority = u2::new(1);

        // PDO mapping
        for pdo in config.pdos.values() {
            if pdo.config.fixed {
                // check that current sdo values are the requested ones
                for (i, sdo) in pdo.sdos.iter().enumerate() {
                    assert_eq!(
                        coe.sdo_read(&pdo.config.item(i), priority).await?,
                        sdo::PdoEntry::new(
                            sdo.field.len.try_into().expect("field too big for a subitem"),
                            sdo.sub.unwrap(),
                            sdo.index,
                            ),
                        "slave {} fixed pdo {}", address, pdo.config.item(i));
                }
            }
            else {
                // TODO: send as a complete SDO rather than subitems if supported
                // pdo size must be set to zero before assigning items
                coe.sdo_write(&pdo.config.len(), priority, 0).await?;
                for (i, sdo) in pdo.sdos.iter().enumerate() {
                    // PDO mapping
                    coe.sdo_write(&pdo.config.item(i), priority, sdo::PdoEntry::new(
                        sdo.field.len.try_into().expect("field too big for a subitem"),
                        sdo.sub.unwrap(),
                        sdo.index,
                        )).await?;
                }
                coe.sdo_write(&pdo.config.len(), priority, pdo.sdos.len() as u8).await?;
            }
        }

        // sync mapping
        for channel in config.channels.values() {

            let mut size = 0;
            // TODO: send as a complete SDO rather than subitems
            // channel size must be set to zero before assigning items
            coe.sdo_write(&channel.config.len(), priority, 0).await?;
            for (j, &pdo) in channel.pdos.iter().enumerate() {
                coe.sdo_write(&channel.config.slot(j as u8), priority, pdo).await?;
                size += config.pdos[&pdo].sdos.iter()
                            .map(|sdo| (sdo.field.len / 8) as u16)
                            .sum::<u16>();
            }
            coe.sdo_write(&channel.config.len(), priority, channel.pdos.len() as u8).await?;

            // enable sync channel
            master.fpwr(address, channel.config.register(), {
                let mut config = registers::SyncManagerChannel::default();
                config.set_address(channel.start);
                config.set_length(size);
                config.set_mode(registers::SyncMode::Buffered);
                config.set_direction(channel.config.direction);
                config.set_dls_user_event(true);
                config.set_watchdog(channel.config.direction == registers::SyncDirection::Write);
                config.set_enable(channel.pdos.len() != 0);
                config
                }).await.one()?;
        }

        // FMMU mapping
        // FMMU entry mode read/write are exclusive, so mapping had to clearly establish which one is used for what
        // the read direction also prevent the memory content to be written before being read by a LRW command, so it is filtering memory accesses
        // no bit alignment is supported now, so bit offsets are 0 at start and 7 at end
        for (i, entry) in config.fmmu.iter().enumerate() {
            assert!(entry.physical + entry.length < physical.end);
            master.fpwr(address, registers::fmmu.entry(i as u8), {
                let mut config = registers::FmmuEntry::default();
                config.set_logical_start_byte(entry.logical + (self.offset as u32));
                config.set_logical_len_byte(entry.length);
                config.set_logical_start_bit(u3::new(0));
                config.set_logical_end_bit(u3::new(7));
                config.set_physical_start_byte(entry.physical);
                config.set_physical_start_bit(u3::new(0));
                config.set_read(entry.direction == SyncDirection::Read);
                config.set_write(entry.direction == SyncDirection::Write);
                config.set_enable(true);
                config
                }).await.one()?;
        }
        
        Ok(())
    }
    /// obtain exclusive access (mutex) to the data buffers
    pub async fn data(&self) -> tokio::sync::MutexGuard<GroupData<'a>> {
        self.data.lock().await
    }
    /// obtain access without locking, exclusivity is guaranteed by self borrowing
    pub fn data_mut(&mut self) -> &mut GroupData<'a> {
        self.data.get_mut()
    }
}
impl<'a> GroupData<'a> {
    pub unsafe fn raw_master(&self) -> &'a RawMaster {self.master}
    
    /// read and write relevant data from master to segment
    pub async fn exchange(&mut self) -> &'_ mut [u8]  {
        // TODO: add a fallback implementation in case the slave does not support *RW commands
        // TODO: offset should be passed as 32 bit address, this requires a modification of RawMaster
        self.master.pdu(PduCommand::LRW, SlaveAddress::Logical, self.offset, self.write.as_mut_slice(), false).await;
        self.read.copy_from_slice(&self.write);
        self.write.as_mut_slice()
    }
    /// read data slice from segment
    pub async fn read(&mut self) -> &'_ mut [u8]  {
        self.master.pdu(PduCommand::LRD, SlaveAddress::Logical, self.offset, self.read.as_mut_slice(), false).await;
        self.read.as_mut_slice()
    }
    /// write data slice to segment
    pub async fn write(&mut self) -> &'_ mut [u8]  {
        self.master.pdu(PduCommand::LWR, SlaveAddress::Logical, self.offset, self.write.as_mut_slice(), false).await;
        self.write.as_mut_slice()
    }
    
    pub fn read_buffer(&mut self) -> &'_ mut [u8] {self.read.as_mut_slice()}
    pub fn write_buffer(&mut self) -> &'_ mut [u8] {self.write.as_mut_slice()}
    
    /// extract a mapped value from the buffer of last received data
    pub fn get<T: PduData>(&self, field: Field<T>) -> T
        {field.get(&self.read)}
    /// pack a mapped value to the buffer for next data write
    pub fn set<T: PduData>(&mut self, field: Field<T>, value: T)
        {field.set(&mut self.write, value)}
}
impl Drop for Group<'_> {
    fn drop(&mut self) {
        self.allocator.internal.lock().unwrap()
            .free.remove(&LogicalSlot {size: self.allocated, position: self.offset});
    }
}
impl fmt::Debug for Group<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<Group at offset: 0x{:x}, {} bytes, {} slaves>",
            self.offset, self.size, self.config.len())
    }
}




/// struct holding configuration informations for multiple slaves, that can be shared between multiple mappings
#[derive(Default, Debug)]
pub struct Config {
    // slave configs are boxed so that they won't move even while inserting in the hashmap
    pub slaves: Mutex<HashMap<u16, Box<RwLock<ConfigSlave>>>>,
}
/// configuration for one slave
#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub struct ConfigSlave {
    pub pdos: HashMap<u16, ConfigPdo>,
    pub channels: HashMap<u16, ConfigChannel>,
    pub fmmu: Vec<ConfigFmmu>,
}
/// configuration for a slave PDO
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigPdo {
    pub config: sdo::Pdo,
    pub sdos: Vec<Sdo>,
}
/// configuration for a slave sync manager channel
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigChannel {
    pub config: sdo::SyncChannel,
    pub pdos: Vec<u16>,
    pub start: u16,
}
/// configuration for a slave FMMU
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigFmmu {
    pub direction: SyncDirection,
    pub length: u16,
    pub physical: u16,
    pub logical: u32,
}


/**
    Convenient struct to create a memory mapping for multiple slaves to the logical memory (responsible for realtime data exchanges).

    It is always slave's physical memory that is mapped to the logical memory. [crate::registers] is giving the set of standard values in the physical memory. Any other values must be configured to be present in the physical memory, such as described in [crate::can::Can].

    ## Principles:

    - This struct (and fellows) provide ways to map every possible thing to the logical memory. Each value-insertion method is returning a [Field] pointing to the position of the mapped value in the contiguous slice configured here (its offset is relative to the slice start and not to the logical memory start).

    - The pushed values will be mapped in the exact order they will be pushed. Depending on the memory layout desired, push calls must be ordered accordingly.

    - The FMMU (Fieldbux Memory Mapping Unit) is hidden from the user and is used to adjust variables order.
*/
pub struct Mapping<'a> {
    /// configuration to modify
    config: &'a Config,
    /// offset in the physical memory
    offset: RefCell<u32>,
    /// default value for logical memory segment (initial value for [GrouData])
    default: RefCell<Vec<u8>>,
    /// keep trace of which slaves are used in this mapping
    slaves: RefCell<HashSet<u16>>,
}
impl<'a> Mapping<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            config,
            offset: RefCell::new(0),
            default: RefCell::new(Vec::new()),
            slaves: RefCell::new(HashSet::new()),
        }
    }
    /// reference to the devices configuration actually worked on by this mapping
    pub fn config(&self) -> &'a Config {
        self.config
    }
    /// create an object for mapping data from a given slave
    ///
    /// data coming for different slaves can interlace, hence multiple slave mapping instances can exist at the same time
    pub fn slave(&self, address: u16) -> MappingSlave<'_>  {
        self.slaves.borrow_mut().insert(address);
        let mut slaves = self.config.slaves.lock().unwrap();
        slaves
            .entry(address)
            .or_insert_with(|| Box::new(RwLock::new(ConfigSlave {
                pdos: HashMap::new(),
                channels: HashMap::new(),
                fmmu: Vec::new(),
                })));
        // uncontroled reference to self and to configuration
        // this is safe since the slave config will not be removed from the hashmap and cannot be moved since it is heap allocated
        // the returned instance holds an immutable reference to self so it cannot be freed
        let slave = unsafe {core::mem::transmute::<_, &Box<RwLock<_>>>( 
                        slaves.get(&address).unwrap() 
                        )};
        MappingSlave {
            config: slave.try_write().expect("slave already in mapping"),
            mapping: self,
            buffer: SLAVE_PHYSICAL_MAPPABLE.start,
            additional: 0,
        }
    }
    /// return the overall data size in this mapping (will increase if more data is pushed in)
    pub fn size(&self) -> u32 {
        self.offset.borrow().clone()
    }
}
/// object allowing to map data from a slave
///
/// data coming from one's slave physical memory shall not interlace (this is a limitation due to this library, not ethercat) so any mapping methods in here are preventing multiple mapping instances
pub struct MappingSlave<'a> {
    mapping: &'a Mapping<'a>,
    config: RwLockWriteGuard<'a, ConfigSlave>,
    /// position in the physical memory mapping region
    buffer: u16,
    /// increment to add to `buffer` once the current mapped channel is done.
    /// this is handling the memory that must be reserved after sync channels to allow the slave to perform buffer swapping
    additional: u16,
}
impl<'a> MappingSlave<'a> {
    /// internal method to increment the logical and physical offsets with the given data length
    /// if the logical offset was changed since the last call to this slave's method (ie. the logical memory contiguity is broken), a new FMMU is automatically configured
    fn insert(&mut self, direction: SyncDirection, length: usize, position: Option<u16>) -> usize {
        let physical = SLAVE_PHYSICAL_MAPPABLE;
        let mut offset = self.mapping.offset.borrow_mut();

        // pick a new position in the physical memory buffer, or pick the given position
        let position = position.unwrap_or_else(|| {
                let position = self.buffer;
                self.buffer += length as u16;
                assert!(self.buffer <= physical.end);
                position
            });
        // create a FMMU if not already existing or if inserted value breaks contiguity
        let change = if let Some(fmmu) = self.config.fmmu.last() {
                fmmu.logical + u32::from(fmmu.length) != *offset
            ||  fmmu.physical + fmmu.length != position
            ||  fmmu.direction != direction
            }
            else {true};
        if change {
            self.config.fmmu.push(ConfigFmmu {
                    direction,
                    length: 0,
                    logical: *offset,
                    physical: position,
                });
        }
        // increment physical and logical memory offsets
        let fmmu = self.config.fmmu.last_mut().unwrap();
        fmmu.length += length as u16;
        let inserted = offset.clone().try_into().unwrap();
        *offset += length as u32;
        inserted
    }
    fn default<T: PduData>(&self, field: Field<T>, value: T) {
        let mut default = self.mapping.default.borrow_mut();
        let range = default.len() .. field.byte + field.len;
        default.extend(range.map(|_| 0));
        field.set(&mut default, value);
    }
    /// increment the value offset with the reserved additional size
    /// this is typically for counting the reserved size of channels triple buffers
    fn finish(&mut self) {
        self.buffer += self.additional;
        // the physical memory buffer must be word-aligned (at least on some devices)
        self.buffer += self.buffer % 2;
        self.additional = 0;
    }
    /// map a range of physical memory, and return its matching range in the logical memory
    pub fn range(&mut self, direction: SyncDirection, range: Range<u16>) -> Range<usize> {
        self.finish();
        let size = usize::from(range.end - range.start);
        let start = self.insert(direction, size, Some(range.start));
        Range {start, end: start+size}
    }
    /// map a field in the physical memory (a register), and return its matching field in the logical memory
    pub fn register<T: PduData>(&mut self, direction: SyncDirection, field: Field<T>) -> Field<T>  {
        self.finish();
        let o = self.insert(direction, field.len, Some(field.byte as u16));
        Field::new(o, field.len)
    }
    /// map a sync manager channel
    pub fn channel(&mut self, sdo: sdo::SyncChannel) -> MappingChannel<'_> {
        self.finish();
        if sdo.register() == registers::sync_manager::interface.mailbox_write()
        || sdo.register() == registers::sync_manager::interface.mailbox_read()
            {panic!("mapping on the mailbox channels (0x1c10, 0x1c11) is forbidden");}
        self.config.channels.insert(sdo.index, ConfigChannel {
            config: sdo,
            pdos: Vec::new(),
            start: self.buffer,
            });
        MappingChannel {
            // uncontroled references to self and to configuration
            // this is safe since the returned object holds a mutable reference to self any way
            entries: unsafe {&mut *(&mut self.config.channels.get_mut(&sdo.index).unwrap().pdos as *mut _)},
            slave: unsafe {&mut *(self as *mut _ as usize as *mut _)},
            direction: sdo.direction,
            capacity: sdo.capacity as usize,
        }

        // TODO: make possible to push an alread existing channel as long as its content is the same
    }
}
pub struct MappingChannel<'a> {
    slave: &'a mut MappingSlave<'a>,
    entries: &'a mut Vec<u16>,
    direction: SyncDirection,
    capacity: usize,
}
impl<'a> MappingChannel<'a> {
    /// add a pdo to this channel, and return an object to map it
    pub fn push(&'a mut self, pdo: sdo::Pdo) -> MappingPdo<'_>  {
        assert!(self.entries.len()+1 < self.capacity);

        self.entries.push(pdo.index);
        self.slave.config.pdos.insert(pdo.index, ConfigPdo {
            config: pdo,
            sdos: Vec::new(),
            });

        MappingPdo {
            // uncontroled reference to self and to configuration
            // this is safe since the returned object holds a mutable reference to self any way
            entries: unsafe {&mut *(&mut self.slave.config.pdos.get_mut(&pdo.index).unwrap().sdos as *mut _)},
            slave: self.slave,
            direction: self.direction,
            capacity: pdo.capacity as usize,
        }

        // TODO: make possible to push an alread existing PDO as long as its content is the same
    }
}
pub struct MappingPdo<'a> {
    slave: &'a mut MappingSlave<'a>,
    entries: &'a mut Vec<Sdo>,
    direction: SyncDirection,
    capacity: usize,
}
impl<'a> MappingPdo<'a> {
    /// add an sdo to this channel, and return its matching field in the logical memory
    pub fn push<T: PduData>(&mut self, sdo: Sdo<T>) -> Field<T> {
        assert!(self.entries.len()+1 < self.capacity);

        self.entries.push(sdo.clone().downcast());
        let len = (sdo.field.len + 7) / 8;
        // the sync channel must allocate 3 times the channel size to allow the slave to perform buffer swapping (the sync channel 3-buffer mode, which is mendatory for realtime operations)
        // so the first thier is reserved using `slave.insert`, and the 2 last using `slave.additional`
        self.slave.additional += 2*len as u16;
        Field::new(self.slave.insert(self.direction, len, None), len)
    }
    /**
        same as [Self::push] but also set an initial value for this SDO in the group buffer.
        This is useful when using a PDO that has more fields than the only desired ones, so we can set them a value and forget them.
    */
    pub fn set<T: PduData>(&mut self, sdo: Sdo<T>, initial: T) -> Field<T> {
        let offset = self.push(sdo);
        self.slave.default(offset, initial);
        offset
    }
}
