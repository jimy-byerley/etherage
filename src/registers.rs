//! structs and consts for every registers in a standard slave's EEPROM. This should be used instead of any hardcoded register value

use bilge::prelude::*;
use crate::data::{self, Field, BitField, ByteArray};



/// ETG.1000.6 table 11
#[bitsize(16)]
#[derive(TryFromBits, Debug)]
pub enum AlError {
   ///  No error Any Current state
    NoError = 0x0000, 
    ///  Unspecified error
    Unspecified = 0x0001, 
    ///  No Memory
    NoMemory = 0x0002, 
    ///  Invalid Device Setup
    InvalidDeviceSetup = 0x0003, 
//     0x0005 Reserved due to compatibility reasons 
    ///  Invalid requested state change 
    InvalidStateRequest = 0x0011, 
    ///  Unknown requested state
    UnknownStateRequest = 0x0012, 
    ///  Bootstrap not supported 
    BootstrapNotSupported = 0x0013, 
    ///  No valid firmware 
    NoValidFirmware = 0x0014, 
    ///  Invalid mailbox configuration
    InvalidMailboxConfig = 0x0015, 
    ///  Invalid mailbox configuration
//     InvalidMailboxConfig = 0x0016, 
    ///  Invalid sync manager configuration
    InvalidSyncConfig = 0x0017, 
    ///  No valid inputs available
    NoInputsAvailable = 0x0018, 
    ///  No valid outputs
    NoValidInputs = 0x0019, 
    ///  Synchronization error
    Synchronization = 0x001A, 
    ///  Sync manager watchdog
    SyncWatchdog = 0x001B, 
    ///  Invalid Sync Manager Types
    InvalidSyncTypes = 0x001C, 
    ///  Invalid Output Configuration
    InvalidOutputConfig = 0x001D, 
    ///  Invalid Input Configuration
    InvalidInputConfig = 0x001E, 
    ///  Invalid Watchdog Configuration
    InvalidWatchdogConfig = 0x001F, 
    ///  Slave needs cold start
    NeedColdStart = 0x0020, 
    ///  Slave needs INIT
    NeedInit = 0x0021, 
    ///  Slave needs PREOP
    NeedPreop = 0x0022, 
    ///  Slave needs SAFEOP
    NeedSafeOp = 0x0023, 
    ///  Invalid Input Mapping
    InvalidInputMapping = 0x0024, 
    ///  Invalid Output Mapping
    InvalidOutputMapping = 0x0025, 
    ///  Inconsistent Settings
    InconsistentSettings = 0x0026, 
    ///  FreeRun not supported
    FreeRunNotSupported = 0x0027, 
    ///  SyncMode not supported
    SyncModeNotSupported = 0x0028, 
    ///  FreeRun needs 3Buffer Mode
    FreeRunNeedsBuffer = 0x0029, 
    ///  Background Watchdog
    BackgroundWatchdog = 0x002A, 
    ///  No Valid Inputs and Outputs
    NoValidIO = 0x002B, 
    ///  Fatal Sync Error
    FatalSync = 0x002C, 
    ///  No Sync Error 
    NoSync = 0x002D, 
    ///  Invalid DC SYNC Configuration
    InvalidDcConfig = 0x0030, 
    ///  Invalid DC Latch Configuration
    InvalidLatchConfig = 0x0031, 
    ///  PLL Error
    PLL = 0x0032, 
    ///  DC Sync IO Error
    DCSyncIO = 0x0033, 
    ///  DC Sync Timeout Error
    DCSyncTimeout = 0x0034, 
    ///  DC Invalid Sync Cycle Time
    DCInvalidPeriod = 0x0035, 
// 0x0036 DC Sync0 Cycle Time
// 0x0037 DC Sync1 Cycle Time
    ///  MBX_AOE
    MailboxAOE = 0x0041, 
    ///  MBX_EOE
    MailboxEOE = 0x0042, 
    ///  MBX_COE
    MailboxCOE = 0x0043, 
    ///  MBX_FOE
    MailboxFOE = 0x0044, 
    ///  MBX_SOE
    MailboxSOE = 0x0045, 
    ///  MBX_VOE
    MailboxVOE = 0x004F, 
    ///  EEPROM no access
    EepromNoAccess = 0x0050, 
    ///  EEPROM Error
    Eeeprom = 0x0051, 
    ///  Slave restarted locally
    SlaveRestarted = 0x0060, 
    ///  Device Identification value updated
    DeviceIdentificationUpdated = 0x0061, 
    // 0x0062 …0  reserved
    ///  Application controller available
    ApplicationAvailable = 0x00F0, 
    // 0x00F0 - 0xFFF:  reserved
    // 0x8000 - 0xFFF:  vendor specific
}


/// used by the slave to inform the master which mailbox protocl can be used with the slave.
/// ETG.1000.6 table 18
#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct MailboxSupport {
    /// ADS over EtherCAT (routing and parallel services)
    aoe: bool,
    /// Ethernet over EtherCAT (tunnelling of Data Link services)
    eoe: bool,
    /// CAN application protocol over EtherCAT (access to SDO)
    coe: bool,
    /// File Access over EtherCAT
    foe: bool,
    /// Servo Drive Profile over EtherCAT
    soe: bool,
    /// Vendor specific protocol over EtherCAT
    voe: bool,
}



/// ETG.1000.4 table 33
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct DLControl {
	/// enables forwarding non-ethercat frames
	forwarding: Forwarding,
	/// 0:permanent setting
	/// 1: temporary use of Loop Control Settings for ~1 second
	temporary: bool,
	reserved: u6,
	ports: [LoopControl; 4],
	/// Buffer between preparation and send. Send will be if buffer is half full (7).
	transmit_buffer_size: u3,
	/// set to true to activate
	low_jitter_ebus: bool,
	reserved: u4,
	alias_enable: bool,
	reserved: u7,
}
data::bilge_pdudata!(DLControl);

#[bitsize(1)]
#[derive(TryFromBits, Debug, Copy, Clone)]
enum Forwarding {
	/// EtherCAT frames are processed, non-EtherCAT frames are forwarded without modification, The source MAC address is not changed for any frame
	Transmit = 0, 
	/// EtherCAT frames are processed, non-EtherCAT frames are destroyed, The source MAC address is changed by the Processing Unit for every frame (SOURCE_MAC[1] is set to 1 – locally administered address).
	Filter = 1,
}
data::bilge_pdudata!(Forwarding);

#[bitsize(2)]
#[derive(TryFromBits, Debug, Copy, Clone)]
enum LoopControl {
	/// closed at “link down”, open with “link up”
	Auto = 0, 
	/// loop closed at “link down”, opened with writing 01 after “link up” (or receiving a valid Ethernet frame at the closed port)
	AutoClose = 1, 
	AlwaysOpen = 2,
	AlwaysClosed = 3,
}
data::bilge_pdudata!(LoopControl);

/// ETG.1000.4 table 34
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct DLStatus {
	/// trie if operational
	dls_user_operational: bool,
	/// true if watchdog not expired
	dls_user_watchdog: bool,
	/// true if activated for at least one port
	extended_link_detection: bool,
	reserved: u1,
	/// indicates physical link on each port
	port_link_status: [bool; 4],
	/// indicates closed loop link status on each port
	port_loop_status: [LoopStatus; 4],
}
data::bilge_pdudata!(DLStatus);

#[bitsize(2)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct LoopStatus {
	/// indicates forwarding on the same port i.e. loop back.
	loop_back: bool,
	signal_detection: bool,
}
data::bilge_pdudata!(LoopStatus);

/// The event registers are used to indicate an event to the DL -user. The event shall be acknowledged if the corresponding event source is read. The events can be masked.
#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct DLSUserEvents {
	/// R1 was written
	r1_change: bool,
	dc: [bool; 3],
	/// event active (one or more Sync manager channels were changed)
	sync_manager_change: bool,
	/// EEPROM command pending
	eeprom_emulation: bool,
	reserved: u2,
	/// mark whether each sync manager channel was accessed
	sync_manager_channel: [bool; 16],
	reserved: u8,
}
data::bilge_pdudata!(DLSUserEvents);

/**
	The external event is mapped to IRQ parameter of all EtherCAT PDUs accessing this slave. If an event is set, and the associated mask is set the corresponding bit in the IRQ parameter of a PDU is set.
	
	ETG.1000.4 table 38
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct ExternalEvent {
	/// dc event 0
	dc0: bool,
	reserved: u1,
	/// dl status register was changed
	dl_status_change: bool,
	/// R3 or R4 was written
	r3_r4_change: bool,
	/// sync manager channel was accessed by slave
	sync_manager_channel: [bool; 8],
	reserved: u1,
}
data::bilge_pdudata!(ExternalEvent);

/// A write to one counter will reset all counters of the group
#[repr(packed)]
#[derive(Clone, Debug)]
struct PortsErrorCount {
	port: [PortErrorCount; 4],
}
data::packed_pdudata!(PortsErrorCount);

#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct PortErrorCount {
	/// counts the occurrences of frame errors (including RX errors within frame)
	frame: u8,
	/// counts the occurrences of RX errors at the physical layer
	physical: u8,
}
data::bilge_pdudata!(PortErrorCount);

#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct LostLinkCount {
	/// counts the occurrences of link down.
	port: [u8; 4],
}
data::bilge_pdudata!(LostLinkCount);

/**
	A write to one counter will reset all Previous Error counters
*/
#[bitsize(48)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct FrameErrorCount {
	/**
	The optional previous indicate a problem on checksum this could be counter is written. The reached. error counter registers contain information about error frames that the predecessor links. As frames with error have a specific type of detected and reported. All previous error counters will be cleared if one counting is stopped for each counter whose maximum value (255) is reached.
	*/
	previous_error_count: [u8; 4],
	/// counts frames with i.e. wrong datagram structure. The counter will be cleared if the counter is written. The counting is stopped when the maximum value (255) is reached.
	malformat_frame_count: u8,
	/// counts occurrence of local problems. The counter will be cleared if the counter is written. The counting is stopped when the maximum value (255) is reached.
	local_problem_count: u8,
}
data::bilge_pdudata!(FrameErrorCount);

/**
	A write will reset the watchdog counters
	
	ETG.1000.4 table 47
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct WatchdogCounter {
	/// counts the expiration of all Sync manager watchdogs.
	sync_manager: u8,
	/// counts the expiration of DL-user watchdogs.
	pdi: u8,
}
data::bilge_pdudata!(WatchdogCounter);


/// ETH.1000.4 table 48
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct SiiAccess {
	owner: SiiOwner,
	lock: bool,
	reserved: u6,
	pdi: bool,
	reserved: u7,
}
data::bilge_pdudata!(SiiAccess);

#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone)]
enum SiiOwner {
	EthercatDL = 0,
	Pdi = 1,
}
data::bilge_pdudata!(SiiOwner);

/** 
    register controling the read/write operations to Slave Information Interface (SII)

	ETG.1000.4 table 49
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
struct SiiControl {
	/// true if SOO is writable
	write_access: bool,
	reserved: u4,
	/**
		- false: Normal operation (DL interfaces to SII)
		- true: DL-user emulates SII
	*/
	eeprom_emulation: bool,
	/// number of bytes per read transaction
	read_size: SiiTransaction,
	/// unit of SII addresses
	address_unit: SiiUnit,
	
	/**
		read operation requested (parameter write) or read operation busy (parameter read)
		To start a new read operation there must be a positive edge on this parameter
	*/
	read_operation: bool,
	/**
		write operation requested (parameter write) or write operation busy (parameter read)
		To start a new write operation there must be a positive edge on this parameter
	*/
	write_operation: bool,
	/**
		reload operation requested (parameter write) or reload operation busy (parameter read)
		To start a new reload operation there must be a positive edge on this parameter
	*/
	reload_operation: bool,
	
	/// checksum error while reading at startup
	checksum_error: bool,
	/// error on reading Device Information
	device_info_error: bool,
	/// error on last command PDI Write only in SII emulation mode
	command_error: bool,
	/// error on last write operation
	write_error: bool,
	
	/// operation is ongoing
	busy: bool,
}
data::bilge_pdudata!(SiiControl);

#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone)]
enum SiiTransaction {
	Bytes4 = 0,
	Bytes8 = 1,
}
#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone)]
enum SiiUnit {
	Byte = 0,
	Word = 1,
}

/// this is not a PduData but a struct transporting the address and number of FMMU registers
/// ETG.1000.4 table 57
pub struct FMMU {
    /// address of the first entry
	pub address: u16,
	/// number of entries
	pub num: u8,
}

impl FMMU {
    /// return an entry of the FMMU
    pub const fn entry(&self, index: u8) -> Field<FMMUEntry>  {
        assert!(index < self.num, "index out of range");
        Field::simple(usize::from(self.address + u16::from(index)*0x10))
    }
}

/**
	The fieldbus memory management unit (FMMU) converts logical addresses into physical addresses by the means of internal address. Thus, FMMUs allow one to use logical addressing for data segments that span several slave devices: one DLPDU addresses data within several arbitrarily distributed devices. The FMMUs optionally support bit wise mapping. A DLE may contain several FMMU entities. Each FMMU entity maps one cohesive logical address space to one cohesive physical address space.
	
	The FMMU consists of up to 16 entities. Each entity describes one memory translation between the logical memory of the EtherCAT communication network and the physical memory of the slave.
	
	ETG.1000.4 table 56
*/
#[bitsize(128)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct FMMUEntry {
	/// start byte in the logical memory
	logical_start_byte: u32,
	/// byte size of the data (rounded to lower value in case of bit-sized data ?)
	logical_len_byte: u16,
	/// offset of the start bit in the logical start byte
	logical_start_bit: u3,
	reserved: u5,
	/// offset of the end bit in the logical start byte
	logical_end_bit: u3,
	reserved: u5,
	
	/// start byte in the physical memory (set by the sync manager)
	physical_start_byte: u16,
	/// start bit in the physical start byte
	physical_start_bit: u3,
	reserved: u5,
	
	/// entity will be used for read service
	read: bool,
	/// entity will be used for write service
	write: bool,
	reserved: u6,
	
	/// enable this FMMU entry, so physical memory will be copied from/to logical memory on read/write
	enable: bool,
	reserved: u7,
	reserved: u24,
}
data::bilge_pdudata!(FMMUEntry);

/// this is not a PduData but a convenience struct transporting the addresses of a sync manager
/// ETG.1000.4 table 59
pub struct SyncManager {
    /// start address of the sync manager (address of the first channel)
    pub address: u16,
    /// number of channels
    pub num: u8,
}

impl SyncManager {
    pub const fn channel(&self, index: u8) -> Field<SyncManagerChannel> {
        assert!(index < self.num, "index out of range");
        Field::simple(usize::from(self.address + u16::from(index)*0x8))
    }
    // return the sync manager channel reserved for mailbox in
    pub const fn mailbox_in(&self) -> Field<SyncManagerChannel>   {self.channel(0)}
    // return the sync manager channel reserved for mailbox out
    pub const fn mailbox_out(&self) -> Field<SyncManagerChannel>   {self.channel(1)}
    // return one of the sync manager channels reserved for mapping
    pub const fn mappable(&self, index: u8) -> Field<SyncManagerChannel>   {self.channel(2+index)}
}

/** 
    The Sync manager controls the access to the DL-user memory. Each channel defines a consistent area of the DL-user memory.

    There are two ways of data exchange between master and PDI:
    
        - Handshake mode (mailbox): one entity fills data in and cannot access the area until the other entity reads out the data.
        - Buffered mode: the interaction between both producer of data and consumer of data is uncorrelated – each entity expects access at any time, always providing the consumer with the newest data.

    ETG.1000.4 table 58
*/
#[bitsize(64)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct SyncManagerChannel {
    /// start address in octets in the physical memory of the consistent DL-user memory area.
    address: u16,
    /// size in octets of the consistent DL -user memory area.
    length: u16,
    /// whether the buffer is used for mailbox or exchange through mapping to the logical memory
    buffer_type: SyncBufferType,
    /// whether the consistent DL -user memory area is read or written by the master.
    direction: SyncBufferDirection,
    
    /// an event is generated if there is new data available in the consistent DL-user memory area which was written by the master (direction write) or if the new data from the DL-user was read by the master (direction read).
    ec_event: bool,
    /// an event is generated if there is new data available in the consistent DL-user memory area which was written by DLS-user or if the new data from the Master was read by the DLS-user.
    dls_user_event: bool,
    /// if the monitoring of an access to the consistent DL-user memory area is enabled.
    watchdog: bool,
    reserved: u1,
    /// if the consistent DL -user memory (direction write) has been written by the master and the event enable parameter is set.
    write_event: bool,
    /// if the consistent DL -user memory (direction read) has been read by the master and the event enable parameter is set.
    read_event: bool,
    reserved: u1,
    
    mailbox_full: bool,
    /// state (buffer number, locked) of the consistent DL-user memory if it is of buffered access type.
    buffer_state: u2,
    read_buffer_open: bool,
    write_buffer_open: bool,
    
    /// activate this channel
    enable: bool,
    /// A change in this parameter indicates a repeat request. This is primarily used to repeat the last mailbox interactions.
    repeat: bool,
    reserved: u4,
    
    /// if the DC 0 Event shall be invoked in case of a EtherCAT write
    dc_event_bus: bool,
    /// if the DC 0 Event shall be invoked in case of a local write
    dc_event_local: bool,
    /// disable this channel for PDI access
    disable_pdi: bool,
    /// indicates a repeat request acknowledge. After setting the value of Repeat in the parameter repeat acknowledge.
    repeat_ack: bool,
    reserved: u6,
}
data::bilge_pdudata!(SyncManagerChannel);

/// ETG.1000.4 table 58
#[bitsize(2)]
#[derive(TryFromBits, Debug)]
enum SyncBufferType {
    Buffered = 0,
    Mailbox = 2,
}
/// ETG.1000.4 table 58
#[bitsize(2)]
#[derive(TryFromBits, Debug)]
enum SyncBufferDirection {
    /// buffer is read by the master
    Read = 0,
    /// buffer is written by the master
    Write = 1,
}

/// ETG.1000.4 table 60
#[repr(packed)]
#[derive(Clone, Debug)]
struct DistributedClock {
    /**
        A write access to port 0 latches the local time (in ns) at receive begin (start first element of preamble) on each port of this PDU in this parameter (if the PDU was received correctly). 
        This array contains the latched receival time on each port.
    */
    received_time: [u32; 4],
    /**
        A write access compares the latched local system time (in ns) at receive begin at the processing unit of this PDU with the written value (lower 32 bit; if the PDU was received correctly), the result will be the input of DC PLL
    */
    system_time: u64,
    /**
        Local time (in ns) at receive begin at the processing unit of a PDU containing a write access to Receive time port 0 (if the PDU was received correctly)
    */
    receive_time_unit: u64,
    /// Offset between the local time (in ns) and the local system time (in ns)
    system_offset: u64,
    /// Offset between the reference system time (in ns) and the local system time (in ns)
    system_delay: u32,
    system_difference: TimeDifference,
    _reserved: [u32; 3],
}
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct TimeDifference {
    /// Mean difference between local copy of System Time and received System Time values
    mean: u31,
    /// true if local copy of system time smaller than received system time
    sign: bool,
}
data::bilge_pdudata!(TimeDifference);



/** registers in physical memory of a slave
	
	this struct does not intend to match any structure defined in the ETG specs, it is only sorting fields pointing to the physical memory of a slave. according to the ETG specs, most of these fields shall exist in each slave's physical memory, others might be optional and the user must check for this before using them.
*/
// const registers = Registers {
// 	address: {
// 		fixed: Field::<u16>::new(0x0010),
// 		alias: Field::<u16>::new(0x0012),
// 	},
// 	dl_control: Field::<DLControl>::new(0x0101),
// 	dl_status: Field::<DLStatus>::new(0x0110),
// 	
// 	dls_user: {
// 		r1: Field::<u8>::simple(0x0120),
// 		r2: Field::<u8>::simple(0x0121),
// 		r3: Field::<u8>::simple(0x0130),
// 		r4: Field::<u8>::simple(0x0131),
// 		r5: Field::<u16>::simple(0x0132),
// 		r6: Field::<u16>::simple(0x0134),
// 		r7: Field::<u8>::simple(0x0140),
// 		copy_r1_r3: BitField::<bool>::new(0x0141*8, 1),
// 		r9: BitField::<u8>::new(0x0141*8+1, 7),
// 		r8: Field::<u8>::simple(0x0150),
// 		
// 		event: Field::<DLSUserEvents>::simple(0x0220),
// 		event_mask: Field::<DLSUserEvents>::simple(0x0202),
// 		watchdog: Field::<u16>::simple(0x0410),
// 	},
// 	
// 	external_event: Field::<ExternalEvent>::simple(0x0210),
// 	external_event_mask: Field::<ExternalEvent>::simple(0x0200),
// 	
// 	ports_errors: Field::<PortsErrorCount>::simple(0x0300),
// 	lost_link_count: Field::<LostLinkCount>::simple(0x0310),
// 	frame_error_count: Field::<FrameErrorCount>::simple(0x0308),
// 	watchdog_divider: Field::<u16>::simple(0x0400),
// 	watchdog_counter: Field::<WatchdogCounter>::simple(0x0442),
// 	
// 	sync_manager: {
// 		/// ETG.1000.6 table 45
// 		watchdog: Field::<u16>::simple(0x0420),
// 		/// ETG.1000.6 table 46
// 		watchdog_status: Field::<bool>::simple(0x0440),
// 	},
// 	
// 	sii: {
// 		access: Field::<SiiAccess>::simple(0x0500),
// 		control: Field::<SiiControl>::simple(0x0502),
// 		/// register contains the address in the slave information interface which is accessed by the next read or write operation (by writing the slave info rmation interface control/status register).
// 		address: Field::<u32>::simple(0x0504),
// 		/// register contains the data (16 bit) to be written in the slave information interface with the next write operation or the read data (32 bit/64 bit) with the last read operation.
// 		data: Field::<u32>::simple(0x0508),
// 	},
// 	
// 	// TODO: MII (Media Independent Interface)
// 	
// 	fmmus: FMMU {address: 0x0600, num: 16},
// 	sync_manager: SyncManager {address: 0x0800, num: 16},
// 	clock: Field::<DistributedClock>::simple(0x0900),
// 	clock_latch: Field::<u32>::simple(0x0900),
// };



pub mod address {
    use super::*;
    
    /// register of the station address, aka the fixed slave address
    /// ETG.1000.4 table 32
    pub const fixed: Field<u16> = Field::simple(0x0010);
    /// slave address alias
    /// ETG.1000.4 table 32
    pub const alias: Field<u16> = Field::simple(0x0012);
}
pub mod dl {
    use super::*;
    
	pub const control: Field<DLControl> = Field::simple(0x0101);
	pub const status: Field<DLStatus> = Field::simple(0x0110);
}
	
pub mod dls_user {
    use super::*;
    
    pub const r1: Field<u8> = Field::simple(0x0120);
    pub const r2: Field<u8> = Field::simple(0x0121);
    pub const r3: Field<u8> = Field::simple(0x0130);
    pub const r4: Field<u8> = Field::simple(0x0131);
    pub const r5: Field<u16> = Field::simple(0x0132);
    pub const r6: Field<u16> = Field::simple(0x0134);
    pub const r7: Field<u8> = Field::simple(0x0140);
    pub const copy_r1_r3: BitField<bool> = BitField::new(0x0141*8, 1);
    pub const r9: BitField<u8> = BitField::new(0x0141*8+1, 7);
    pub const r8: Field<u8> = Field::simple(0x0150);
    
    pub const event: Field<DLSUserEvents> = Field::simple(0x0220);
    pub const event_mask: Field<DLSUserEvents> = Field::simple(0x0202);
    pub const watchdog: Field<u16> = Field::simple(0x0410);
}
	
pub const external_event: Field<ExternalEvent> = Field::simple(0x0210);
pub const external_event_mask: Field<ExternalEvent> = Field::simple(0x0200);
	
pub const ports_errors: Field<PortsErrorCount> = Field::simple(0x0300);
pub const lost_link_count: Field<LostLinkCount> = Field::simple(0x0310);
pub const frame_error_count: Field<FrameErrorCount> = Field::simple(0x0308);
pub const watchdog_divider: Field<u16> = Field::simple(0x0400);
pub const watchdog_counter: Field<WatchdogCounter> = Field::simple(0x0442);
	
pub mod sync_manager {
    use super::*;

    /// ETG.1000.6 table 45
    pub const watchdog: Field<u16> = Field::simple(0x0420);
    /// ETG.1000.6 table 46
    pub const watchdog_status: Field<bool> = Field::simple(0x0440);
	pub const interface: SyncManager = SyncManager {address: 0x0800, num: 16};
}

pub const mailbox_buffers: [Field<[u8; 0x100]>; 3] = [
	Field::simple(0x1000),
	Field::simple(0x1100),
	Field::simple(0x1200),
	];
	
	/*
	sii: {
		access: Field::<SiiAccess>::simple(0x0500),
		control: Field::<SiiControl>::simple(0x0502),
		/// register contains the address in the slave information interface which is accessed by the next read or write operation (by writing the slave info rmation interface control/status register).
		address: Field::<u32>::simple(0x0504),
		/// register contains the data (16 bit) to be written in the slave information interface with the next write operation or the read data (32 bit/64 bit) with the last read operation.
		data: Field::<u32>::simple(0x0508),
	},
	
	// TODO: MII (Media Independent Interface)
	
	fmmus: FMMU {address: 0x0600, num: 16},
	sync_manager: SyncManager {address: 0x0800, num: 16},
	clock: Field::<DistributedClock>::simple(0x0900),
	clock_latch: Field::<u32>::simple(0x0900),
};
*/
