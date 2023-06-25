/*! 
Convenient structures to read/write the slave's dictionnary objects (SDO) and configure mappings...);
*/

use crate::{
// 	slave::Slave,
	data::{self, Field, BitField, PduData, Storage},
	registers,
	};
use core::fmt;
use bilge::prelude::*;

pub use crate::registers::SyncDirection;


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
	
	const pub fn downcast(self) -> Sdo { Sdo{
        index: self.index,
        sub: self.sub,
        field: BitField::new(self.field.bit, self.field.len),
	}}
	const pub fn into_sub(&self) -> Sdo<T> { 
        match self.sub {
            SdoPart::Complete => Sdo{
                index: self.index,
                sub: SdoPart::Sub(0),
                field: self.field,
                },
            SdoPart::Sub => self,
        }
	}
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
		write!(f, "Sdo<{}> {{index: 0x{:x}, sub: {:?}, field: {{0x{:x}, {}}}}}", core::any::type_name::<T>(), self.index, self.sub, self.field.bit, self.field.len)
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




const FIRST_SYNC_CHANNEL: u16 = 0x1c10;



// standard SDO definitions, that shall exist on all device implementing CoE

pub const error: Sdo<DeviceError> = Sdo::sub(0x1001, 0, 0);
pub mod device {
    use super::*;
    
    pub const ty: Sdo<DeviceType> = Sdo::sub(0x1000, 0, 0);
    pub const name: Sdo = Sdo::sub(0x1008, 0, 0);
    pub const hardware_version: Sdo = Sdo::sub(0x1009, 0, 0);
    pub const software_version: Sdo = Sdo::sub(0x100a, 0, 0);
}
// const identity: Sdo<record> = Sdo::complete(0x0018);
const receive_pdos: PdoMappings = PdoMappings {index: 0x1600, num: 512};
const transmit_pdos: PdoMappings = PdoMappings {index: 0x1a00, num: 512};
const transmit_pdos_invalid: PdoInvalids = PdoInvalids {index: 0x603e, num: 254};
const sync_manager: SyncManager = SyncManager {index: 0x1c10, num: 32};
const synchronization: Synchronization = Synchronization {index: 0x1c30, num: 32};

/**
    dictionnary entries defined for devices supporting CIA.402, defined in ETG.6010
    
    CIA.402 is the standard for controling servodrives and stepperdrives (for electric motors) in ethercat and canopen.
    
    These entries will not be present on devices not supporting CIA.402, and may not all be present on devices implementing only a subset of CIA.402.
    Some entries defined here depend on the actual device type and will not be present on devices implementing CIA.402 but not concerned by the sdo meaning (eg. items regarding a stepper motor will not be present on devices controling a brushless motor)
*/
pub mod cia402 {
    use super::*;
    
    pub const controlword: Sdo<ControlWord> = Sdo::complete(0x6040);
    pub const statusword: Sdo<StatusWord> = Sdo::complete(0x6041);
    pub const error: Sdo<u16> = Sdo::complete(0x603f);
    pub const supported_mode: Sdo<SupportedModes> = Sdo::complete(0x6502);
    /// supported synchronization functions in the device, fields are true if the matching flag in [StatusWord] are supported
    pub const supported_sychronization: Sdo<SynchronizationSetting> = Sdo::complete(0x60d9);
    /**
        The control unit can write the requested mode to 60DA h during start-up. Following reactions from the drive shall be accepted:
        
        1. Drive accepts values
        
            The control unit and the drive shall use the requested synchronization function
        
        2. If none of the synchronization functions are supported and therefore the object does not exist the drive sends abort with Abort Code 06020000h “The object does not exist in the object directory”.
        
            The control unit shall not use the synchronization function
        
        3. If the requested mode is not supported the drive sends abort with Abort Code 06090030h “Value range of parameter exceeded”.
        
            The control unit shall not use the requested synchronization function.
            
            The drive shall not use any synchronization function, i.e. 60DA h = 0000h
    */
    pub const enabled_sychronization: Sdo<SynchronizationSetting> = Sdo::complete(0x60da);
    
    pub mod target {
        use super::*;
    
        /// The Operation mode can be switched by writing this. This can be done via PDO communication or via SDO communication
        pub const mode: Sdo<OperationMode> = Sdo::complete(0x6060);
        pub const position: Sdo<i32> = Sdo::complete(0x607a);
        pub const velocity: Sdo<i32> = Sdo::complete(0x60ff);
        pub const torque: Sdo<i16> = Sdo::complete(0x6071);
    }
    pub mod offset {
        use super::*;
    
        pub const position: Sdo<i32> = Sdo::complete(0x60b0);
        pub const velocity: Sdo<i32> = Sdo::complete(0x60b1);
        pub const torque: Sdo<i16> = Sdo::complete(0x60b2);
    }
    pub mod current {
        use super::*;
    
        pub const mode: Sdo<OperationMode> = Sdo::complete(0x6061);
        pub const position: Sdo<i32> = Sdo::complete(0x6064);
        pub const velocity: Sdo<i32> = Sdo::complete(0x606c);
        pub const torque: Sdo<i16> = Sdo::complete(0x6077);
    }
    /** 
        These values limits the torque the actuator can deliver.
    
        ETG.6010 7.2
    */
    pub mod max_torque {
        use super::*;
        
        /**        
            The limiting of the torque will be stated in the bit 11 "internal limit active" in the statusword.
            
            The lowest of the limiting values is effective
        */
        pub const global: Sdo<u16> = Sdo::complete(0x6072);
        /** 
            indicate the configured maximum positive torque in the motor. The value shall be given per thousand of rated torque
            
            Positive torque takes effect in the case of:
            - motive operation is positive velocity
            - regenerative operation is negative velocity
        */
        pub const positive: Sdo<u16> = Sdo::complete(0x60e0);
        /** 
            indicate the configured maximum negative torque in the motor. The value shall be given per thousand of rated torque
            
            Negative torque takes effect in the case of:
            - motive operation is negative velocity
            - regenerative operation is positive velocity
        */
        pub const negative: Sdo<u16> = Sdo::complete(0x60e1);
    }
    /// ETG.6010 7.3
    pub mod homing {
        use super::*;
        
        pub const offset: Sdo<> = Sdo::complete(0x607c);
        /**
            ETG.6010 Table 27: Homing methods
            
            | method | Description |
            |--------|-------------|
            | 1 | Homing on negative limit switch and index pulse
            | 2 | Homing on positive limit switch and index pulse
            | 3, 4 | Homing on positive home switch and index pulse
            | 5, 6 | Homing on negative home switch and index pulse
            | 7 .. 14 | Homing on home switch and index pulse
            | 15, 16 | Reserved
            | 17 .. 30 | Homing without index pulse
            | 31, 32 | Reserved
            | 33, 34 | Homing on index pulse
            | 35 | Homing on current position – obsolete
            | 36 | Homing with touch-probe
            | 37 | Homing on current position
            
            See ETG.6010 7.3 for more details
        */
        pub const method: Sdo<u16> = Sdo::complete(0x6098);
        pub const velocity: Sdo<u32> = Sdo::complete(0x6099);
        pub const acceleration: Sdo<u32> = Sdo::complete(0x609a);
        pub const supported: SdoList<bool> = SdoList {index: 0x60e3};
    }
    /// ETG.6010 7.4
    pub mod touch {
        // TODO
    }
    
    // TODO:  see what is Factor Group, ETG.6010 8
    
    /// A drive may support several sensor interfaces. The information coming from this/these additional sensors should be given here
    pub mod sensors {
        pub const position: SdoList<i32> = SdoList<i32> {index: 0x60e4};
        pub const position_encoder_increments: SdoList<u32> = SdoList {index: 0x60e6};
        pub const position_motor_revolutions: SdoList<u32> = SdoList {index: 0x60eb};
        
        pub const velocity: SdoList<i32> = SdoList {index: 0x60e5};
        pub const velocity_encoder_increments: SdoList<u32> = SdoList {index: 0x60e7};
        pub const velocity_motor_revolutions: SdoList<u32> = SdoList {index: 0x60ec};
        
        pub const gear_ratio_motor: SdoList<u32> = SdoList {index: 0x60e8};
        pub const gear_ratio_shaft: SdoList<u32> = SdoList {index: 0x60ed};
        pub const feed: SdoList<u32> = SdoList {index: 0x60e9};
        pub const shaft: SdoList<u32> = SdoList {index: 0x60ee};
    }
    
    pub const position_limit: Sdo<PositionLimits> = Sdo::complete(0x607b);
    pub const position_limit_software: Sdo<> = Sdo::complete(0x607d);
    pub const position_mode: Sdo<PositioningMode> = Sdo::complete(0x60f2);
    
    pub const following_error_current: Sdo<i32> = Sdo::complete(0x60f4);
    pub const following_error_window: Sdo<> = Sdo::complete(0x6065);
    pub const following_error_timeout: Sdo<> = Sdo::complete(0x6066);
    pub const max_velocity: Sdo<u32> = Sdo::complete(0x6080);
    pub const max_rated_torque: Sdo<u16> = Sdo::complete(0x6076);
    
    pub const polarity: Sdo<> = Sdo::complete(0x607e);
    pub const sensor_velocity: Sdo<i32> = Sdo::complete(0x6069);
    
    pub const motion_profile: Sdo<> = Sdo::complete(0x6086);
    pub const interpolation_time_period: Sdo<> = Sdo::complete(0x60c2);
    
    /// This object shall indicate the electrical commutation angle for the space vector modulation. The value 16 shall be given in 360°/2 , whereby the electrical angle is used. Table 13 specifies the object description, and Table 14 specifies the entry description.
    pub const commutation_angle: Sdo<u16> = Sdo::complete(0x60ea);
    
    pub mod quick_stop {
        use super::*;
    
        pub const deceleration: Sdo<> = Sdo::complete(0x6085);
        pub const option: Sdo<> = Sdo::complete(0x605a);
    }
    
    // csp
    // statusword.12 = following_command  (mendatory)
    /*   
        Drive follows the command value shall be zero if the drive does not follow the target value (position, velocity or torque) because of local reasons (internal set-point settings), e.g. if a local Input is configured to a halt function or a safety function prevents the drive in Operational to follow the target set point. The control device shall evaluate the bit. The Bit 12 shall be set if the drive is in state Operation enabled and follows the target and set-point values of the control device. In all other cases it shall be zero. If the bit is not supported it shall be fix set to 1 in the statusword.
    */

    // statusword.10 = reached_command
    /*   
        used in Profile position mode as "target reached" information. In csp the new target position is given cyclically by the control device. This bit can be used as Status Toggle information to indicate if the device provides updated input data. The bit shall be toggled with every update of the input process data. If object 60D9 h is supported, the Status Toggle function can be enabled or disabled.
        The Following Error functionality is usually not supported by the drive in csp mode therefore Bit 13 can be used to extend the Status Toggle information to a 2-Bit Input Cycle Counter. Object 60D9h and 60DAh shall be supported and used to enable or disable the Input Cycle Counter functionality.
    */
    
    //csv
    // statusword.12 = following_command  (mendatory)
    /*
        Drive follows the command value shall be zero if the drive does not follow the target value (position, velocity or torque) because of local reasons (internal set-point settings), e.g. if a local Input is configured to a halt function or if a safety function prevents the drive in Operational to follow the target set point. The control device shall evaluate the bit. The Bit 12 shall be set if the drive is in state Operation enabled and follows the target and set-point values of the control device. In all other cases it shall be zero. If the bit is not supported it shall be fix set to 1 in the statusword.
    */
    // statusword.10 = reached_command
    /*
        In csv mode Bit 10 can be used as Status Toggle information to indicate if the device provides updated input data. The bit shall be toggled with every update of the input process data. If object 60D9h is supported, the Status Toggle function can be enabled or disabled.
        Bit 13 can be used to extend the Status Toggle information to a 2-Bit Input Cycle Counter. Object 60D9h and 60DAh shall be supported and used to enable or disable the Input Cycle Counter functionality.
    */
    
    //cst
    // statusword.12 = following_command  (mendatory)
    /*
        The Bit 12 Drive follows the command value shall be zero if the drive does not follow the target value (position, velocity or torque) because of local reasons (internal set-point settings), e.g. if a local Input is configured to a halt function or if a safety function prevents the drive in Operational to follow the target set point. The control device shall evaluate the bit. The Bit 12 shall be set if the drive is in state Operation enabled and follows the target and set-point values of the control device. In all other cases it shall be zero. If the bit is not supported it shall be fix set to 1 in the statusword.
        
        NOTE: If Bit 12 is not supported and is not set to 1, the control device will not run the drive, because it expects that the drive does not follow the command.
    */
    // statusword.10 = reached_command
    /*
        In cst mode Bit 10 can be used as Status Toggle information to indicate if the device provides updated input data. The bit shall be toggled with every update of the input process data. If object 60D9h is supported, the Status Toggle function can be enabled or disabled.
        Bit 13 can be used to extend the Status Toggle information to a 2-Bit Input Cycle Counter. Object 60D9h and 60DAh shall be supported and used to enable or disable the Input Cycle Counter functionality.
    */
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
    /// bit size of the subitem value
    bitsize: u8,
    /// mapped sdo subindex (it is not possible to map complete sdo, so this field must always be set)
    sub: u8,
    /// mapped sdo index
    index: u16,
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
        SyncChannel { 
            index: self.index + u16::from(i), 
            direction: match i%2 {
                0 => SyncDirection::Write,
                1 => SyncDirection::Read,
                _ => unreachable!(),
                }, 
            num: 254,
            }
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
	/// whether this channel is to be read or written by the master, this might be set by the user if the slave supports it.
	pub direction: SyncDirection,
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
    pub fn register(&self) -> Field<registers::SyncManagerChannel> {
        registers::sync_manager::interface.channel((self.index - FIRST_SYNC_CHANNEL).try_into().unwrap())
    }
}
impl fmt::Debug for SyncChannel {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "SyncChannel {{index: 0x{:x}, num: {}}}", self.index, self.num)
	}
}

/**
    a SDO containing a boolean for each PDO, read true if the matching PDO has an invalid mapping
    
    ETG.6010 table 6
*/
pub struct PdoInvalids {
    pub index: u16,
    pub num: u8,
}
impl PdoInvalids {
    pub fn len(&self) -> Sdo<u8>  {Sdo::sub(self.index, 0)}
    pub fn slot(&self, i: u8) -> Sdo<bool>  {Sdo::sub_with_size(self.index, (i+1), (i+1)*8, 8)}
}




#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct DeviceType {
    /// manufacturer specific
    pub mode: u8,
    /// services supported by the device
    pub ty: DriveType,
    /// prodile identifier: 402 for a cia402 compliant device
    pub profile: u16,
}
data::bilge_pdudata!(DeviceType, u32);

#[bitsize(4)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
pub struct DriveType {
    reserved: u1,
    /// device is an ethercat servodrive
    pub servo: bool,
    /// device is an ethercat stepper drive
    pub stepper: bool,
    /// device supports ETG.6100 safety drive profile
    pub safety: bool,
    reserved: u4,
}

/**
    identify the source of actual errors on the device. several source can cause errors at the same time.
    
    ETG.1000.6 table 69
*/
#[bitsize(8)]
pub struct DeviceError {
    /// generic error, no particular source
    generic: bool,
    /// error in current control
    current: bool,
    /// error in voltage control
    voltage: bool,
    temperature: bool,
    communication: bool,
    /// error specific to the device profile in use
    device_profile: bool,
    reserved: u1,
    /// manufactorer-specific error
    manufacturer: bool,
}

/**
bit structure of a status word

| Bit |  Meaning | Presence |
|-----|----------|----------|
| 0	| Ready to switch on	| M
| 1	| Switched on	| M
| 2	| Operation enabled	| M
| 3	| Fault	| M
| 4	| Voltage enabled	| O
| 5	| Quick stop	| O
| 6	| Switch on disabled	| M
| 7	| Warning	| O
| 8	| Manufacturer specific	| O
| 9	| Remote	| O
| 10	| Operation mode specific	| O
| 11	| Internal limit active	| C
| 12	| Operation mode specific (Mandatory for csp, csv, cst mode)	| O
| 13	| Operation mode specific	| O
| 14-15	| Manufacturer specific	| O

bit 10 `reached_command`, bit 12 `following_command`, bit 13 `cycle`  are operation specific, so do not rely on it in modes that do not use them. See [OperationMode] for more details.
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct StatusWord {
    pub ready_switch_on: bool,
    pub switched_on: bool,
    pub operation_enabled: bool,
    pub fault: bool,
    pub voltage_enabled: bool,
    pub quick_stop: bool,
    pub switch_on_disabled: bool,
    pub warning: bool,
    reserved: u1,
    pub remote: bool,
    pub reached_command: bool,
    pub limit_active: bool,
    pub following_command: bool,
    pub cycle: bool,
    reserved: u2,
}
data::bilge_pdudata!(StatusWord, u16);

impl fmt::Display for StatusWord {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "StatusWord{{")?;
		for (active, mark) in [ (self.ready_switch_on(), "rtso"),
								(self.switched_on(), "so"),
								(self.operation_enabled(), "oe"),
								(self.fault(), "f"),
								(self.voltage_enabled(), "ve"),
								(self.quick_stop(), "qs"),
								(self.switch_on_disabled(), "sod"),
								(self.warning(), "w"),
								(self.remote(), "r"),
								(self.limit_active(), "la"),
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

/**
Control word of a servo drive

| Bit	|	Category	|   Meaning	|
|-------|---------------|-----------|
| 0	|	M	|	Switch on |
| 1	|	M	|	Enable voltage |
| 2	|	O	|	Quick stop |
| 3	|	M	|	Enable operation |
| 4 – 6	|	O	|	Operation mode specific |
| 7	|	M	|	Fault reset |
| 8	|	O	|	Halt |
| 9	|	O	|	Operation mode specific |
| 10	|	O	|	reserved |
| 11 – 15	|	O	|	Manufacturer specific |
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct ControlWord {
    pub switch_on: bool,
    pub enable_voltage: bool,
    pub quick_stop: bool,
    pub enable_operation: bool,
    pub homing: bool,
    reserved: u2,
    pub reset_fault: bool,
    pub halt: bool,
    pub specific: bool,
    reserved: u1,
    reserved: u5,
}
data::bilge_pdudata!(ControlWord, u16);

impl fmt::Display for ControlWord {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ControlWord{{") ?;
		for (active, mark) in [ (self.switch_on(), "so"),
								(self.enable_voltage(), "ev"),
								(self.quick_stop(), "qs"),
								(self.enable_operation(), "eo"),
								(self.reset_fault(), "rf"),
								(self.halt(), "h"),
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


/// servodrive control-loop type
#[bitsize(8)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum OperationMode {
    /// actuator power disabled
    #[default]
	Off = 0,
	/// PP
	ProfilePosition = 1,
	/// VL
	Velocity = 2,
	/// PV
	ProfileVelocity = 3,
	/// TQ
	TorqueProfile = 4,
	/// HM
	Homing = 6,
	/// IP
	InterpolatedPosition = 7,
	
	/**
        CSP (Cyclic Synchronous Position)
        
        If the following error is calculated in the control device it is afflicted with a dead-time. Therefore the calculation of the following error in the drive might have a better quality.
        The following error value shall only be evaluated in state Operation enabled.
        After a Reset the set point should be set to the actual value so that the following error is zero.
        In the csp mode the Halt bit (bit 8) of the controlword shall be ignored because the halt function is controlled by the control device.
        Bit 5 and 6 of the controlword can be used as Output Cycle Counter. This 2-Bit counter can be used by the control device to indicate if updated output data are available. The counter shall be incremented with every update of the output process data. Object 60D9h and 60DAh shall be supported and used to enable or disable the Output Cycle Counter functionality.
    */
	SynchronousPosition = 8,
	/** 
        CSV (Cyclic Synchronous Velocity)
        
        In the csv mode the Halt bit (bit 8) of the Controlword shall be ignored because the halt function is controlled by the control device.
        Bit 5 and 6 of Controlword can be used as Output Cycle Counter. This 2-Bit counter can be used by the control device to indicate if updated output data are available. The counter shall be incremented
    */
	SynchronousVelocity = 9,
	/**
        CST (Cyclic Synchronous Torque)
        
        In the cst mode the Halt bit (bit 8) of the Controlword shall be ignored because the halt function is controlled by the control device.
        Bit 5 and 6 of Controlword can be used as Output Cycle Counter. This 2-Bit counter can be used by the control device to indicate if updated output data are available. The counter shall be incremented with every update of the output process data. Object 60D9h and 60DAh shall be supported and used to enable or disable the Output Cycle Counter functionality.
    */
	SynchronousTorque = 10,
	/**
        CSTCA (Cyclic Synchronous Torque mode with Commutation Angle)
        
        With this mode, the trajectory generator is located in the control device, not in the drive device. In cyclic synchronous manner, it provides a commutation angle and a target torque to the drive device, which performs current control and space vector modulation. Optionally, an additive torque value can be provided by the control system in order to allow two instances to set up the torque. Measured by sensors, the drive device could provide actual values for position or may provide velocity and torque to the control device.
    
        This mode can be used for example
        - to find the commutation angle during commissioning
        - to check the function (increments, zero signal) and direction of the sensor
        - Use of external sensor interface to calculate commutation angle (in that case there might be no internal feedback to Torque control)
        
        In the cstca mode the Halt bit (bit 8) of the Controlword shall be ignored because the halt function is controlled by the control device.
        Bit 5 and 6 of Controlword can be used as Output Cycle Counter. This 2-Bit counter can be used by the control device to indicate if updated output data are available. The counter shall be incremented with every update of the output process data. Object 60D9h and 60DAh shall be supported and used to enable or disable the Output Cycle Counter functionality.

        In cstca mode Bit 10 can be used as Status Toggle information to indicate if the device provides updated input data. The bit shall be toggled with every update of the input process data. If object 60D9h is supported, the Status Toggle function can be enabled or disabled.
        Bit 13 can be used to extend the Status Toggle information to a 2-Bit Input Cycle Counter. Object 60D9h and 60DAh shall be supported and used to enable or disable the Input Cycle Counter functionality. The Bit 12 Drive follows the command value shall be zero if the drive does not follow the target value (position, velocity or torque) because of local reasons (internal set-point settings), e.g. if a local Input is configured to a halt function or if a safety function prevents the drive in Operational to follow the target set point. The control device shall evaluate the bit. The Bit 12 shall be set if the drive is in state Operation enabled and follows the target and set-point values of the control device. In all other cases it shall be zero. If the bit is not supported it shall be fix set to 1 in the statusword.
    */
	SynchronousTorqueCommutation = 11,
}
data::bilge_pdudata!(OperationMode, u8);

/**
    provide information on the supported drive modes. each field match a standard variant of [OperationMode]. manufacturer-specific modes can be checked in the remainning reserved bits of this struct.
    
    ETG.6010 figure 15
*/
#[bitsize(32)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct SupportedModes {
    pub profile_position: bool,
    pub velocity: bool,
    pub profile_velocity: bool,
    pub torque_profile: bool,
    pub homing: bool,
    pub interpolated_position: bool,
    
    pub synchronous_position: bool,
    pub synchronous_velocity: bool,
    pub synchronous_torque: bool,
    pub synchronous_torque_commutation: bool,
    
    // manufacturer-specific modes
    reserved: u22,
}
data::bilge_pdudata!(SupportedModes, u32);

/**
    ETG.6010 figure 16, 17
*/
#[bitsize(32)]
pub struct SynchronizationSetting {
    ///  Status Toggle bit in csp, csv, cst and cstca mode supported/enabled
    pub toggle: bool,
    /// Status Toggle can be extended to a 2-Bit Input Cycle Counter in the Status word in csp, csv, cst and cstca mode
    pub input_cycle: bool,
    /// Output Cycle Counter in csp, csv, cst and cstca mode supported/enabled
    pub output_cycle: bool,
    reserved: u29,
}

pub struct HomingMethods {
    pub index: u16,
    pub num: 254,
}
impl HomingMethods {
    pub fn len(&self) -> Sdo<u8>    {Sdo::sub(self.index, 0)}
    pub fn method(&self, id: u8) -> Sdo<bool>   {Sdo::sub(self.index, id+1, (id+1)*8)}
}

#[bisize(16)]
struct Positioning {
    relative: bool,
    /// change immediately option
    cio: u2,
    /// request response option
    rro: u2,
    /// rotary axis direction option
    rado: PositioningMode,
    ip: u3,
    reserved: u3,
    manufacturer: u1,
}
/**
    | negative | positive | effect |
    |----------|----------|--------|
    |     0    |    0     | Normal positioning similar to linear axis. If reaching or exceeding the position range limits (607Bh) the input value shall wrap automatically to the other end of the range
    |     0    |    1     | Positioning only in negative direction; if target position is higher than actual position, axis moves over “Min position limit“ to target position
    |     1    |    0     | Positioning only in positive direction; if target position is lower than actual position, axis moves over “Max position limit“ to target position
    |     1    |    1     | Positioning with the shortest way to the target position. Special condition: If the difference between actual value and target position in a 360° system is 180°, the axis will move in positive direction.
*/
#[bitsize(2)]
struct PositioningMode {
    /// if target position is higher than actual position, axis moves over “Min position limit“ to target position
    closest_negative: bool,
    /// if target position is lower than actual position, axis moves over “Max position limit“ to target position
    closest_positive: bool,
}
