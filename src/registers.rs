/*!
    structs and consts for every registers in a standard slave's RAM. This should be used instead of any hardcoded register value.
    
    The goal of this file is to gather all physical memory registers at one place, so what you see here is exactly what you can expect in a slave, no more, no less.
    
    Some registers are partially redundant, this is because we can use some field pointing to a big struct and other fields pointing to only parts of the same struct.
*/

use core::fmt;
use bilge::prelude::*;
use crate::data::{self, Field, BitField, Storage};

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

    pub const information: Field<DLInformation> = Field::simple(0x0000);
	pub const control: Field<DLControl> = Field::simple(0x0100);
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

/** 
    SM (Sync Managers) are used for configuring and controling two distinct things:
    - mailbox exchanges (CoE, FoE, ...)
    - pdo exchanges (copying PDO data to slave's physical memory)
*/
pub mod sync_manager {
    use super::*;

    /// ETG.1000.6 table 45
    pub const watchdog: Field<u16> = Field::simple(0x0420);
    /// ETG.1000.6 table 46
    pub const watchdog_status: Field<bool> = Field::simple(0x0440);
	pub const interface: SyncManager = SyncManager {address: 0x0800, num: 16};
}

/// SII (Slave Information Interface) allows to retreive declarative informations about a slave (like a manifest) like product code, vendor, etc as well as slave boot-up configs
pub mod sii {
    use super::*;
    
	pub const access: Field<SiiAccess> = Field::simple(0x0500);
	pub const control: Field<SiiControl> = Field::simple(0x0502);
	/// register contains the address in the slave information interface which is accessed by the next read or write operation (by writing the slave info rmation interface control/status register).
	pub const address: Field<u16> = Field::simple(0x0504);
	/// register contains the data (16 bit) to be written in the slave information interface with the next write operation or the read data (32 bit/64 bit) with the last read operation.
	pub const data: Field<[u8; 8]> = Field::simple(0x0508);
	
	/// agregates [const@control] and [const@address] for optimized bandwith
	pub const control_address: Field<SiiControlAddress> = Field::simple(control.byte);
	/// agregates [const@control] and [const@address] and [const@data] for optimized bandwith
	pub const control_address_data: Field<SiiControlAddressData> = Field::simple(control.byte);
}
	
// TODO: MII (Media Independent Interface)

/// FMMU (Fieldbus Memory Management Unit) is controling the mapping (copy) for a slave's physical memory from/to logical memory
pub const fmmu: FMMU = FMMU {address: 0x0600, num: 16};
pub const clock: Field<DistributedClock> = Field::simple(0x0900);
pub const clock_latch: Field<u32> = Field::simple(0x0900);

/// AL (Application Layer) registers are controling the communication state of a slave
pub mod al {
    use super::*;
    
    pub const control: Field<AlControlRequest> = Field::simple(dls_user::r1.byte);
    pub const response: Field<AlControlResponse> = Field::simple(dls_user::r3.byte);
    pub const error: Field<AlError> = Field::simple(dls_user::r6.byte);
    pub const status: Field<AlStatus> = Field::simple(dls_user::r3.byte);
    pub const pdi: Field<AlPdiControlType> = Field::simple(dls_user::r7.byte);
    pub const sync_config: Field<AlSyncConfig> = Field::simple(dls_user::r8.byte);
}



/// ETG.1000.6 table 9
#[bitsize(8)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct AlControlRequest {
    /// requested state of communication
    pub state: AlMixedState,
    /// if true, parameter change of the [AlStatus::changed] will be reset
    pub ack: bool,
    /// request of id instead of error code in [al::error]
    pub request_id: bool,
    reserved: u2,
}
data::bilge_pdudata!(AlControlRequest, u8);

/// ETG.1000.6 table 10
#[bitsize(8)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct AlControlResponse {
    /// formerly requested state of communication
    pub state: AlMixedState,
    /**
    - false: State transition successful
    - true: State transition not successful
    */
    pub error: bool,
    /// if true, ID value is present in R6
    pub id: bool,
    reserved: u2,
}
data::bilge_pdudata!(AlControlResponse, u8);

/// ETG.1000.6 table 12
#[bitsize(8)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct AlStatus {
    /// current state of communication
    pub state: AlMixedState,
    /// true if requested by [AlControlRequest::ack]
    pub changed: bool,
    reserved: u3,
}
data::bilge_pdudata!(AlStatus, u8);

/**
    the current operation state on one device.
    
    This is the enum version, useful when communicating with one slave only
    
    Except [Self::Bootstrap], changing to any mode can be requested from any upper mode or from the preceding one.

    ETG.1000.6 table 9
*/
#[bitsize(4)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum AlState {
    /**
        Transitional state meaning the slave is booting up and ready for nothing yet. The slave should normally reach the [Self::Init] state within seconds.
        
        It cannot be requested, nor changed while it is active.
    */
    Bootstrap = 3,
    /**
        The init mode allows to set many communication registers, like the salve address, the mailbox setup, etc.
        
        This mode should be used at the beginning of a communication. Only registers can be used.
    */
    Init = 1,
    /**
        the pre operational mode allows mailbox communication, which is mendatory to configure some slaves before realtime operations. Most functions are enabled but not realtime.
        
        Communication setup via registers is no more allowed in this mode.
    */
    PreOperational = 2,
    /**
        Mode allowing realtime operations, except that commands sent to the slaves via its mapping will not be executed.
        
        This is a kind of read-only temporary mode before [Self::Operational], that can be useful for initializing control loops on the master side while their outputs are ignored.
        
        Mapping is no more allowed in this state, nor communication setup via registers.
    */
    SafeOperational = 4,
    /**
        Realtime operations running
        
        The master has full access to the slave's effector functions. slaves might expect the master to regularly refresh its commands.
        
        Mapping is no more allowed in this state, nor communication setup via registers.
    */
    Operational = 8,
}

/**
	gather the current operation states on several devices
	this struct does not provide any way to know which slave is in which state
	
	This is the bitfield version, useful when communicating with multiple slaves (broadcast PDUs)
	
    ETG.1000.6 table 9
*/
#[bitsize(4)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct AlMixedState {
    /// one slave at least is in [AlState::Init]
	pub init: bool,
	/// one slave at least is in [AlState::PreOperational]
	pub pre_operational: bool,
	/// one slave at least is in [AlState::SafeOperational]
	pub safe_operational: bool,
	/// one slave at least is in [AlState::Operational]
	pub operational: bool,
}

impl fmt::Display for AlMixedState {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}{{", core::any::type_name::<Self>()) ?;
		for (active, mark) in [ (self.init(), "init"),
								(self.pre_operational(), "pre"),
								(self.safe_operational(), "safe"),
								(self.operational(), "op"),
								] {
			write!(f, " ")?;
			if active {
				write!(f, "{}", mark)?;
			} else {
				for _ in 0 .. mark.len() {write!(f, " ")?;}
			}
		}
		write!(f, "}}")?;
		Ok(())
	}
}

impl TryFrom<AlMixedState> for AlState {
    type Error = &'static str;
    fn try_from(state: AlMixedState) -> Result<Self, Self::Error> {
        Self::try_from(u4::from(state)).map_err(|e|  "cannot unwrap when not only 1 state in mix")
    }
}
impl From<AlState> for AlMixedState {
    fn from(state: AlState) -> Self {
        Self::from(u4::from(state))
    }
}

/// ETG.1000.6 table 11
#[bitsize(16)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
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
    ///  Invalid mailbox configuration for switching to [AlState::Init]
    InvalidMailboxConfigBoot = 0x0015, 
    ///  Invalid mailbox configuration for switching to [AlState::PreOperational]
    InvalidMailboxConfigPreop = 0x0016, 
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
    ///  Phase Link Lock Error
    PLL = 0x0032, 
    ///  Distributed Clock Sync IO Error
    DCSyncIO = 0x0033, 
    ///  Distributed Clock Sync Timeout Error
    DCSyncTimeout = 0x0034, 
    ///  Distributed Clock Invalid Sync Cycle Time
    DCInvalidPeriod = 0x0035, 
// 0x0036 Distributed Clock Sync0 Cycle Time
// 0x0037 Distributed Clock Sync1 Cycle Time
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
    ///  raised when switching to PreOperational but SII access owner has not been switched to PDI
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
    // 0x00F0 - 0xFFFF:  reserved
    // 0x8000 - 0xFFFF:  vendor specific
    Specific = 0xffff,
}
data::bilge_pdudata!(AlError, u16);

/// ETG.1000.6 table 13
#[bitsize(9)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct AlPdiControlType {
    /// Type specific (see ETG.1000.3 DL information parameter)
    pub pdi: u8,
    /**
        - false: AL Management will be done by an application Controller
        - true: AL Management will be emulated (AL status follows directly AL control)
    */
    pub strict: bool,
}
data::bilge_pdudata!(AlPdiControlType, u9);

/// ETG.1000.6 table 15
#[bitsize(8)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct AlSyncConfig {
    /// controller specific
    pub signal_conditioning_sync0: u2,
    pub enable_signal_sync0: bool,
    pub enable_interrupt_sync0: bool,
    
    /// controller specific
    pub signal_conditioning_sync1: u2,
    pub enable_signal_sync1: bool,
    pub enable_interrupt_sync1: bool,
}
data::bilge_pdudata!(AlSyncConfig, u8);


/// ETG.1000.4 table 31
#[bitsize(80)]
pub struct DLInformation {
    /// type of the slave controller
    pub ty: u8,
    /// (major revision) revision of the slave controller.
    pub revision: u8,
    /// (minor revision) build number of the slave controller
    pub build: u16,
    
    /// Number of supported FMMU entities  (1 to 10)
    pub fmmus: u8,
    /// Number of supported Sync Manager channels (1 to 10)
    pub sync_managers: u8,
    
    /// RAM size in koctet, means 1024 octets (1-60)
    pub ram_size: u8,
    /// tells which port are present on a slave
    pub ports: [PortDescriptor; 4],
    
    /**
        - `false`: bit operation supported
        - `true`: bit operation not supported This feature bit does not affect mappability of SM.WriteEvent flag (MailboxIn)
    */
    pub fmmu_bit_operation_not_supported: bool,
    /**
        - `true`: shall only be used for legacy reasons. Reserved registers may not be written, reserved registers may not be read when out of register area (refer to documentation of specific slave controller (ESC) 
        - `false`: no restriction in register access
    */
    pub reserved_registers_not_supported: bool,
    pub dc_supported: bool,
    pub dc_range: DcRange,
    pub ebus_low_jitter: bool,
    pub ebus_enhanced_link_detection: bool,
    /// true if available
    pub mii_enhanced_link_detection: bool,
    /// if true, frames with modified FCS (additional nibble) should be counted separately in RX-Error Previous counter
    pub separate_fcs_errors: bool,
    /// true if available. This feature refers to registers 0x981\[7:3\], 0x0984
    pub dc_sync_activation_enhanced: bool,
    
    /// if true, `LRW` is not supported
    pub logical_exchange_not_supported: bool,
    /// if true, `BRW`, `APRẀ`, `FPRW` is not supported
    pub physicial_exchange_not_supported: bool,
    
    /**
        - 0: not active
        - 1: active, FMMU 0 is used for RxPDO (no bit mapping) FMMU 1 is used for TxPDO (no bit mapping) FMMU 2 is used for Mailbox write event bit of Sync manager 1 Sync manager 0 is used for write mailbox Sync manager 1 is used for read mailbox Sync manager 2 is used as Buffer for incoming data Sync manager 3 is used as Buffer for outgoing data
    */
    pub special_fmmu_config: bool,
    reserved: u4,
}
data::bilge_pdudata!(DLInformation, u80);

#[bitsize(2)]
#[derive(FromBits, Debug, Copy, Clone)]
pub enum PortDescriptor {
    NotImplemented = 0b00,
    NotConfigured = 0b01,
    /// ethernet bus
    Ebus = 0b10,
    /// Machine Abstraction Interface (MII/RMII)
    Mii = 0b11,
}
#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone)]
pub enum DcRange {
    /// default
    Bit32 = 0,
    /// 64 bit for system time, system time offset and receive time processing unit
    Bit64 = 1,
}



/// used by the slave to inform the master which mailbox protocl can be used with the slave.
/// ETG.1000.6 table 18
#[bitsize(16)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct MailboxSupport {
    /// ADS over EtherCAT (routing and parallel services)
    pub aoe: bool,
    /// Ethernet over EtherCAT (tunnelling of Data Link services)
    pub eoe: bool,
    /// CAN application protocol over EtherCAT (access to SDO)
    pub coe: bool,
    /// File Access over EtherCAT
    pub foe: bool,
    /// Servo Drive Profile over EtherCAT
    pub soe: bool,
    /// Vendor specific protocol over EtherCAT
    pub voe: bool,
    reserved: u10,
}
data::bilge_pdudata!(MailboxSupport, u16);



/// ETG.1000.4 table 33
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct DLControl {
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
data::bilge_pdudata!(DLControl, u32);

#[bitsize(1)]
#[derive(TryFromBits, Debug, Copy, Clone)]
pub enum Forwarding {
	/// EtherCAT frames are processed, non-EtherCAT frames are forwarded without modification, The source MAC address is not changed for any frame
	Transmit = 0, 
	/// EtherCAT frames are processed, non-EtherCAT frames are destroyed, The source MAC address is changed by the Processing Unit for every frame (SOURCE_MAC\[1\] is set to 1 – locally administered address).
	Filter = 1,
}
data::bilge_pdudata!(Forwarding, u1);

#[bitsize(2)]
#[derive(TryFromBits, Debug, Copy, Clone)]
pub enum LoopControl {
	/// closed at “link down”, open with “link up”
	Auto = 0, 
	/// loop closed at “link down”, opened with writing 01 after “link up” (or receiving a valid Ethernet frame at the closed port)
	AutoClose = 1, 
	AlwaysOpen = 2,
	AlwaysClosed = 3,
}
data::bilge_pdudata!(LoopControl, u2);

/// ETG.1000.4 table 34
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct DLStatus {
	/// trie if operational
	pub dls_user_operational: bool,
	/// true if watchdog not expired
	pub dls_user_watchdog: bool,
	/// true if activated for at least one port
	pub extended_link_detection: bool,
	reserved: u1,
	/// indicates physical link on each port
	pub port_link_status: [bool; 4],
	/// indicates closed loop link status on each port
	pub port_loop_status: [LoopStatus; 4],
}
data::bilge_pdudata!(DLStatus, u16);

#[bitsize(2)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct LoopStatus {
	/// indicates forwarding on the same port i.e. loop back.
	pub loop_back: bool,
	pub signal_detection: bool,
}
data::bilge_pdudata!(LoopStatus, u2);

/// The event registers are used to indicate an event to the DL -user. The event shall be acknowledged if the corresponding event source is read. The events can be masked.
#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct DLSUserEvents {
	/// R1 was written
	pub r1_change: bool,
	pub dc: [bool; 3],
	/// event active (one or more Sync manager channels were changed)
	pub sync_manager_change: bool,
	/// EEPROM command pending
	pub eeprom_emulation: bool,
	reserved: u2,
	/// mark whether each sync manager channel was accessed
	pub sync_manager_channel: [bool; 16],
	reserved: u8,
}
data::bilge_pdudata!(DLSUserEvents, u32);

/**
	The external event is mapped to IRQ parameter of all EtherCAT PDUs accessing this slave. If an event is set, and the associated mask is set the corresponding bit in the IRQ parameter of a PDU is set.
	
	ETG.1000.4 table 38
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct ExternalEvent {
	/// dc event 0
	pub dc0: bool,
	reserved: u1,
	/// dl status register was changed
	pub dl_status_change: bool,
	/// R3 or R4 was written
	pub r3_r4_change: bool,
	/// sync manager channel was accessed by slave
	pub sync_manager_channel: [bool; 8],
	reserved: u4,
}
data::bilge_pdudata!(ExternalEvent, u16);

/// A write to one counter will reset all counters of the group
/// ETG.1000.4 table 40
#[repr(packed)]
#[derive(Clone, Debug, Default)]
pub struct PortsErrorCount {
	pub port: [PortErrorCount; 4],
}
data::packed_pdudata!(PortsErrorCount);

#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Default)]
pub struct PortErrorCount {
	/// counts the occurrences of frame errors (including RX errors within frame)
	pub frame: u8,
	/// counts the occurrences of RX errors at the physical layer
	pub physical: u8,
}
data::bilge_pdudata!(PortErrorCount, u16);

#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct LostLinkCount {
	/// counts the occurrences of link down.
	pub port: [u8; 4],
}
data::bilge_pdudata!(LostLinkCount, u32);

/**
	A write to one counter will reset all Previous Error counters
*/
#[bitsize(48)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct FrameErrorCount {
	/**
	The optional previous indicate a problem on checksum this could be counter is written. The reached. error counter registers contain information about error frames that the predecessor links. As frames with error have a specific type of detected and reported. All previous error counters will be cleared if one counting is stopped for each counter whose maximum value (255) is reached.
	*/
	pub previous_error_count: [u8; 4],
	/// counts frames with i.e. wrong datagram structure. The counter will be cleared if the counter is written. The counting is stopped when the maximum value (255) is reached.
	pub malformat_frame_count: u8,
	/// counts occurrence of local problems. The counter will be cleared if the counter is written. The counting is stopped when the maximum value (255) is reached.
	pub local_problem_count: u8,
}
data::bilge_pdudata!(FrameErrorCount, u48);

/**
	A write will reset the watchdog counters
	
	ETG.1000.4 table 47
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone)]
pub struct WatchdogCounter {
	/// counts the expiration of all Sync manager watchdogs.
	pub sync_manager: u8,
	/// counts the expiration of DL-user watchdogs.
	pub pdi: u8,
}
data::bilge_pdudata!(WatchdogCounter, u16);


/// ETH.1000.4 table 48
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Default)]
pub struct SiiAccess {
	pub owner: SiiOwner,
	/// setting this will reset access to SII
	pub lock: bool,
	pub reserved: u6,
	/// PDI access active
	pub pdi: bool,
	pub reserved: u7,
}
data::bilge_pdudata!(SiiAccess, u16);

#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SiiOwner {
	EthercatDL = 0,
	Pdi = 1,
}
data::bilge_pdudata!(SiiOwner, u1);

/** 
    register controling the read/write operations to Slave Information Interface (SII)

	ETG.1000.4 table 49
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Default)]
pub struct SiiControl {
	/// true if SII is writable
	pub write_access: bool,
	reserved: u4,
	/**
		- false: Normal operation (DL interfaces to SII)
		- true: DL-user emulates SII
		
		cannot be set by the master
	*/
	pub eeprom_emulation: bool,
	/// number of bytes per read transaction, cannot be set by master
	pub read_size: SiiTransaction,
	/// unit of SII addresses, cannot be set by master
	pub address_unit: SiiUnit,
	
	/**
		read operation requested (parameter write) or read operation busy (parameter read)
		To start a new read operation there must be a positive edge on this parameter
		
		This parameter will be written from the master to start the read operation of 32 bits/64 bits in the slave information interface. This parameter will be read from the master to check if the read operation is finished. 
	*/
	pub read_operation: bool,
	/**
		write operation requested (parameter write) or write operation busy (parameter read)
		To start a new write operation there must be a positive edge on this parameter
		
		This parameter will be written from the master to start the write operation of 16 bits in the slave information interface. This parameter will be read from the master to check if the write operation is finished. There is no consistence gu arantee for write operation. A break down during write can produce inconsistent values and should be avoided. 
	*/
	pub write_operation: bool,
	/**
		reload operation requested (parameter write) or reload operation busy (parameter read)
		To start a new reload operation there must be a positive edge on this parameter
		
		This parameter will be written from the master to start the reload operation of the first 128 bits in the slave information interface. This parameter will be read from the master to check if the reload operation is finished
	*/
	pub reload_operation: bool,
	
	/// checksum error while reading at startup
	pub checksum_error: bool,
	/// error on reading Device Information
	pub device_info_error: bool,
	/**
        error on last SII request
        
        writable only in SII emulation mode
    */
	pub command_error: bool,
	/// error on last write operation
	pub write_error: bool,
	
	/// operation is ongoing
	pub busy: bool,
}
data::bilge_pdudata!(SiiControl, u16);

#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SiiTransaction {
	Bytes4 = 0,
	Bytes8 = 1,
}
#[bitsize(1)]
#[derive(FromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SiiUnit {
	Byte = 0,
	Word = 1,
}

#[repr(packed)]
#[derive(Debug, Copy, Clone)]
pub struct SiiControlAddress {
    pub control: SiiControl,
    pub address: u16,
}
data::packed_pdudata!(SiiControlAddress);

#[repr(packed)]
#[derive(Debug, Copy, Clone)]
pub struct SiiControlAddressData {
    pub control: SiiControl,
    pub address: u16,
    pub reserved: u16,
    pub data: [u8; 2],
}
data::packed_pdudata!(SiiControlAddressData);

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
    pub fn entry(&self, index: u8) -> Field<FmmuEntry>  {
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
#[derive(TryFromBits, DebugBits, Copy, Clone, Default)]
pub struct FmmuEntry {
	/// start byte in the logical memory
	pub logical_start_byte: u32,
	/// byte size of the data (rounded to lower value in case of bit-sized data ?)
	pub logical_len_byte: u16,
	/// offset of the start bit in the logical start byte
	pub logical_start_bit: u3,
	reserved: u5,
	/// offset of the end bit in the logical start byte
	pub logical_end_bit: u3,
	reserved: u5,
	
	/// start byte in the physical memory (set by the sync manager)
	pub physical_start_byte: u16,
	/// start bit in the physical start byte
	pub physical_start_bit: u3,
	reserved: u5,
	
	/// entity will be used for read service
	pub read: bool,
	/// entity will be used for write service
	pub write: bool,
	reserved: u6,
	
	/// enable this FMMU entry, so physical memory will be copied from/to logical memory on read/write
	pub enable: bool,
	reserved: u7,
	reserved: u24,
}
data::bilge_pdudata!(FmmuEntry, u128);

/// this is not a PduData but a convenience struct transporting the addresses of a sync manager
/// ETG.1000.4 table 59
pub struct SyncManager {
    /// start address of the sync manager (address of the first channel)
    pub address: u16,
    /// number of channels
    pub num: u8,
}

impl SyncManager {
    pub fn channel(&self, index: u8) -> Field<SyncManagerChannel> {
        assert!(index < self.num, "index out of range");
        Field::simple(usize::from(self.address + u16::from(index) * (core::mem::size_of::<SyncManagerChannel>() as u16) ))
    }
    // return the sync manager channel reserved for mailbox in
    pub fn mailbox_write(&self) -> Field<SyncManagerChannel>   {self.channel(0)}
    // return the sync manager channel reserved for mailbox out
    pub fn mailbox_read(&self) -> Field<SyncManagerChannel>   {self.channel(1)}
    // return one of the sync manager channels reserved for mapping
    pub fn mappable(&self, index: u8) -> Field<SyncManagerChannel>   {self.channel(2+index)}
}

/** 
    The Sync manager controls the access to the DL-user memory. Each channel defines a consistent area of the DL-user memory.

    There is two ways of data exchange between master and PDI:
    - Handshake mode (mailbox): one entity fills data in and cannot access the area until the other entity reads out the data.
    - Buffered mode: the interaction between both producer of data and consumer of data is uncorrelated – each entity expects access at any time, always providing the consumer with the newest data.

    ETG.1000.4 table 58
*/
#[bitsize(64)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Default)]
pub struct SyncManagerChannel {
    /// start address in octets in the physical memory of the consistent DL-user memory area.
    /// multiple sync manager channels with overlapping memory ranges are not supported.
    pub address: u16,
    /// size in octets of the consistent DL -user memory area.
    pub length: u16,
    /// whether the buffer is used for mailbox or exchange through mapping to the logical memory
    pub mode: SyncMode,
    /// whether the consistent DL -user memory area is read or written by the master.
    pub direction: SyncDirection,
    
    /// an event is generated if there is new data available in the consistent DL-user memory area which was written by the master (direction write) or if the new data from the DL-user was read by the master (direction read).
    pub ec_event: bool,
    /// an event is generated if there is new data available in the consistent DL-user memory area which was written by DLS-user or if the new data from the Master was read by the DLS-user.
    pub dls_user_event: bool,
    /// if the monitoring of an access to the consistent DL-user memory area is enabled.
    pub watchdog: bool,
    reserved: u1,
    /// if the consistent DL -user memory (direction write) has been written by the master and the event enable parameter is set.
    pub write_event: bool,
    /// if the consistent DL -user memory (direction read) has been read by the master and the event enable parameter is set.
    pub read_event: bool,
    reserved: u1,
    
    /// true if there is data waiting to be read (by master or slave) in the buffer
    pub mailbox_full: bool,
    /// state (buffer number, locked) of the consistent DL-user memory if it is of buffered access type.
    pub buffer_state: u2,
    pub read_buffer_open: bool,
    pub write_buffer_open: bool,
    
    /// activate this channel
    pub enable: bool,
    /// A change in this parameter indicates a repeat request. This is primarily used to repeat the last mailbox interactions.
    pub repeat: bool,
    reserved: u4,
    
    /// if the DC 0 Event shall be invoked in case of a EtherCAT write
    pub dc_event_bus: bool,
    /// if the DC 0 Event shall be invoked in case of a local write
    pub dc_event_local: bool,
    /// disable this channel for PDI access
    pub disable_pdi: bool,
    /// indicates a repeat request acknowledge. After setting the value of Repeat in the parameter repeat acknowledge.
    pub repeat_ack: bool,
    reserved: u6,
}
data::bilge_pdudata!(SyncManagerChannel, u64);

/// ETG.1000.4 table 58
#[bitsize(2)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SyncMode {
    Buffered = 0,
    Mailbox = 2,
}
/// ETG.1000.4 table 58
#[bitsize(2)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum SyncDirection {
    /// sync manager buffer is read by the master
    Read = 0,
    /// sync manager buffer is written by the master
    Write = 1,
}

/// ETG.1000.4 table 60
#[repr(packed)]
#[derive(Clone, Debug)]
pub struct DistributedClock {
    /**
        A write access to port 0 latches the local time (in ns) at receive begin (start first element of preamble) on each port of this PDU in this parameter (if the PDU was received correctly). 
        This array contains the latched receival time on each port.
    */
    pub received_time: [u32; 4],
    /**
        A write access compares the latched local system time (in ns) at receive begin at the processing unit of this PDU with the written value (lower 32 bit; if the PDU was received correctly), the result will be the input of DC PLL
    */
    pub system_time: u64,
    /**
        Local time (in ns) at receive begin at the processing unit of a PDU containing a write access to Receive time port 0 (if the PDU was received correctly)
    */
    pub receive_time_unit: u64,
    /// Offset between the local time (in ns) and the local system time (in ns)
    pub system_offset: u64,
    /// Offset between the reference system time (in ns) and the local system time (in ns)
    pub system_delay: u32,
    pub system_difference: TimeDifference,
    reserved: [u32; 3],
}
data::packed_pdudata!(DistributedClock);

#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
pub struct TimeDifference {
    /// Mean difference between local copy of System Time and received System Time values
    pub mean: u31,
    /// true if local copy of system time smaller than received system time
    pub sign: bool,
}
data::bilge_pdudata!(TimeDifference, u32);




