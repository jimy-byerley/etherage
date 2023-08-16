/// Master normally has two different synchronization modes
/// - Cyclic mode
/// - DC Mode
/// In Cyclic mode the master send the process data frame cyclically. The process data frame may be sent with different cycle times.

use crate::{
    registers::{
        dc::{*, self},
        isochronous::{*, self}, DLStatus, dl, AlState, dls_user
    },
    EthercatError,
    rawmaster::{RawMaster, PduAnswer}, Slave, sdo::{SyncMangerFull, self}, sdo::Sdo,
};

use bilge::prelude::*;
use chrono;
use futures_concurrency::future::Join;
use core::option::Option;
use std::collections::HashMap;
use tokio::time::{Duration, self};
use thread_priority::*;

/// Default frame count used to set static drift synchronization
const STATIC_FRAME_COUNT : usize = 15000;
/// Default time (in µs) used to trigger system time synchronization
const CONTINOUS_TIMELAPS : u64 = 2000000;

#[derive(Default, Debug, PartialEq)]
enum DCCorrectionType {
    #[default]
    Static,
    Continuous,
}

#[derive(Default, Debug, PartialEq)]
enum MasterSyncType {
    #[default]
    Cyclic,
    Dc
}

/// Enumerator to define and implement all synchronization type suppported by the etherCAT protocol
/// ETG1020 table 66
#[derive(Default, Debug, PartialEq, PartialOrd)]
pub enum SlaveSyncType {
    /// Not synchronized
    #[default]
    Free,
    /// Synchronize to SM2 event channel
    Sm2,
    /// Synchronize to SM3 event channel
    Sm3,
    /// SM2, Shift Input Latch
    Sm2Shif,
    /// SM3, Shift Input Latch
    Sm3Shift,
    /// DC
    DcSync0,
    /// DC, Shift of Outputs Valid with shift
    DcSyncShift,
    /// DC, Shift Outputs Valid and Input Latch with SYNC 1
    DcSyncShiftSync1,
    /// DC, Shift of Input Latch with SYNC1
    DcSync1,
    /// Subordinated Application Controller Cycles - DC, Shift Outputs Valid / Input Latch
    Subordinate,
}

impl SlaveSyncType {
    /// Return the 0x1C32 and 0x1C33 value for the synchronisation mode provide in parameter <br>
    /// If index is not used, value is set to 0 by default <br>
    /// Return array with [0x1C32 SI 01; 0x1C33 SI 01]
    pub const fn get_byte(m : SlaveSyncType) -> [u8;2] {
        return match m {
            SlaveSyncType::Free => [0x00u8, 0x00u8],
            SlaveSyncType::Sm2 => [0x01u8, 0x22u8],
            SlaveSyncType::Sm3 => [0x00u8, 0x01u8],
            SlaveSyncType::Sm2Shif => [0x01u8, 0x22u8],
            SlaveSyncType::Sm3Shift => [0x00u8, 0x01u8],
            SlaveSyncType::DcSync0 => [0x02u8, 0x02u8],
            SlaveSyncType::DcSyncShift => [0x02u8, 0x03u8],
            SlaveSyncType::DcSync1 => [0x03u8, 0x03u8],
            SlaveSyncType::DcSyncShiftSync1 => [0x03u8, 0x02u8],
            SlaveSyncType::Subordinate => [0x03u8, 0x02u8],
        }
    }

    fn from_field(v : u16) -> SlaveSyncType {
        let dc_sm: crate::BitField<u8> = crate::data::BitField::new(1,1);
        let dc_typ: crate::BitField<u8> = crate::data::BitField::new(2,4);
        let dc_shift: crate::BitField<u8> = crate::data::BitField::new(5,2);
        let data : &[u8] = &v.to_be_bytes()[0..];
        let mut slv_type : SlaveSyncType = match dc_typ.get(&data) {
            0x00 => SlaveSyncType::Free,
            0x01 => if dc_shift.get(data) == 0x00 { SlaveSyncType::DcSync0 } else { SlaveSyncType::DcSyncShift },
            0x02 => if dc_shift.get(data) == 0x00 { SlaveSyncType::DcSync1 } else { SlaveSyncType::DcSyncShiftSync1 },
            _ => SlaveSyncType::Subordinate,
        };
        if slv_type == SlaveSyncType::Free && dc_sm.get(data) == 0x01 {
            slv_type = SlaveSyncType::Sm2;
        }
        return  slv_type;
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub enum ClockStatus {
    #[default]
    Registering,
    Initialized,
    Active
}

struct SlaveInfo {
    index: u16,
    clock_range: bool,
    is_desync : bool,
    clock: DistributedClock,
    isochronous : isochronous::Isochronous,
    clock_type : SlaveSyncType,
}

impl SlaveInfo{
    fn new(idx: u16) -> Self {
        Self {
            index: idx,
            clock_range: true,
            is_desync : false,
            clock: DistributedClock::new(),
            isochronous : isochronous::Isochronous::new(),
            clock_type : SlaveSyncType::Free,
        }
    }
}

/// Helper to configurate to configurate **basic** DC synchronisation <br>
/// To use more **advanced** clock use SDO with 1xC32 index on SM2 channel and 1xC33 index on SM3 channel
#[derive(Debug, Clone, Copy)]
pub struct SlaveClockConfigHelper {
    /// The interrupt generation will start when the lower 32bits of the system time will reach this value (ns)
    pub cyclic_start_time : Option<u32>,
    /// Sync 0 cyclic time (ns) - if *Option::None*, disable Sync0 pulse generation
    pub sync_0_time : Option<u32>,
    /// Sync 1 cyclic time (ns) - if *Option::None*, disable Sync1 pulse generation
    pub sync_1_time : Option<u32>,
    /// Pulse debug time option - Taken from SII (Slave Information Interface) - Multiple of **10ns**
    pub pulse : Option<u16>,
}

impl SlaveClockConfigHelper {
    pub fn new(cyclic_op_time: Option<u32>, sync_0_time : Option<u32>, sync_1_time : Option<u32>, pulse : Option<u16>) -> Self {
        Self { cyclic_start_time: cyclic_op_time, sync_0_time, sync_1_time, pulse }
    }
    /// Default implementation: sync_0_time = 20000ns [0x1e8480], sync_1_time = none, pulse : none,
    pub fn default() -> Self {
        Self { sync_0_time: Some(2000000), sync_1_time: Option::None, cyclic_start_time: None, pulse: Some(1) }
    }
}

/// Use to implement advanced clock synchronization through syncmanager (mailbox is required)
pub struct SynchronizationSdoHelper {}

impl SynchronizationSdoHelper {

    /// Send data for configurate the specified synchronisation chanel. Send sdo for the register `0x1C32`+ channel`.
    pub async fn send(sync_type : SlaveSyncType, cycle_time : Option<u32>, shift_time : Option<u32>, get_cycle_time : Option<(bool,bool)>, slv : &'_ Slave<'_>, channel : u16) ->
         Result<(),EthercatError<&'static str>> {
        let status : AlState = slv.state().await;
        if status == AlState::SafeOperational || status == AlState::Operational {

            if channel > 32 { return Err(EthercatError::Protocol("Channel to high")); }

            let mut canoe: tokio::sync::MutexGuard<'_, crate::can::Can<'_>> = slv.coe().await;
            //let param_couter: Sdo<u8> = Sdo::<u8>::sub(0x1C30 + u16::from(sm_channel), 0, 0);
            let sdo_sync: sdo::Synchronization = sdo::Synchronization {
                index : 0x1C32 + channel
            };
            let typ = match sync_type {
                SlaveSyncType::Free => sdo::SyncType::none(),
                SlaveSyncType::Sm2 => sdo::SyncType::synchron(),
                SlaveSyncType::Sm3 => sdo::SyncType::sync_manager(channel as u8),
                SlaveSyncType::DcSync0 => sdo::SyncType::dc_sync0(),
                SlaveSyncType::DcSync1 => sdo::SyncType::dc_sync1(),
                _ => sdo::SyncType::none(),
            };
            let sdo_sync_typ: Sdo<sdo::SyncType> = sdo_sync.ty();
            let sdo_cycle: Sdo<u32> = sdo_sync.period();
            let sdo_shift: Sdo<u32> = sdo_sync.shift();
            let get_lt = sdo_sync.toggle_lt();

            //Check capability or return error
            let smf : SyncMangerFull = Self::receive(slv, channel).await.expect("Error channel not available");
            let capability : sdo::SyncSupportedMode = sdo::SyncSupportedMode::from(smf.supported_sync_type);
            if bool::from(capability.sm())  && sync_type == SlaveSyncType::Sm2 {
                return Err(EthercatError::Protocol("Synchronization not supported by this slave"));
            }
            if bool::from(capability.dc_sync0()) && sync_type == SlaveSyncType::DcSync0 {
                return Err(EthercatError::Protocol("Synchronization not supported by this slave"));
            }
            if bool::from(capability.dc_sync1()) && sync_type == SlaveSyncType::DcSync1 {
                return Err(EthercatError::Protocol("Synchronization not supported by this slave"));
            }
            if bool::from(capability.dc_fixed()) && sync_type == SlaveSyncType::DcSyncShiftSync1 {
                return Err(EthercatError::Protocol("Synchronization not supported by this slave"));
            }

            //Write synchronization sdo
            canoe.sdo_write(&sdo_sync_typ, u2::new(1), typ).await;
            if cycle_time.is_some() {
                canoe.sdo_write(&sdo_cycle, u2::new(1), cycle_time.unwrap()).await; }
            if shift_time.is_some() {
                canoe.sdo_write(&sdo_shift, u2::new(1), shift_time.unwrap()).await; }
            if bool::from(capability.dc_sync0()) || bool::from(capability.dc_sync1()) || bool::from(capability.dc_fixed()) && get_cycle_time.is_some() {
                let mut data : sdo::SyncCycleTimeDsc = Default::default();
                data.set_measure_local_time(u1::from(get_cycle_time.unwrap().0));
                data.set_reset_event_counter(u1::from(get_cycle_time.unwrap().1));
                canoe.sdo_write(&get_lt, u2::new(1), data).await; }
        }
        return Ok(());
    }

    /// Receive synchronization sdo for the specific channel. Receive sdo for the register `0x1C32`+ channel`.
    /// Return a None if `channel > 32`
    pub async fn receive(slv : &'_ Slave<'_>, channel : u16 ) -> Option<SyncMangerFull> {
        if channel > 32 {
            return Option::None; }
        let sdo_id = Sdo::<SyncMangerFull>::complete(0x1C32 + channel);
        let mut ans = slv.coe().await;
        return Some(ans.sdo_read(&sdo_id, u2::new(1)).await);
    }
}

pub struct SyncClock<'a> {
    // Raw master reference
    master: &'a RawMaster,
    // Index of the reference slave in the "physical" loop
    ref_p_idx: u16,
    // Index of the reference slave in the internal array
    ref_l_idx : Option<usize>,
    // Time shift between master local time and reference local time (minus transmition delay)
    offset : u64,
    // Delay transmision between master and reference
    delay : u32,
    // Shift delay used to advance transmission frame and allow "input data" to be available at sm3 event
    shift : u32,
    // Type of DC used
    dc_correction_typ : DCCorrectionType,
    // Array of registerd slave
    slaves: Vec<SlaveInfo>,
    // DC control cycle time
    cycle_time: u64,
    // Number of frame used to prevent static drift
    drift_frm_count : usize,
    // Current status of the SyncClock structure
    status : ClockStatus,
    // Allow master to send message on DC clock tick
    event : &'a tokio::sync::Notify,
    // Current master synchronization type
    typ : MasterSyncType,
}

impl<'a> SyncClock<'a> {
// ================================    Ctor   =================================
    /// Initialize a new disribution clock
    pub fn new(master: &'a RawMaster, event : &'a tokio::sync::Notify) -> Self {
        Self {
            master,
            ref_p_idx: 0,
            ref_l_idx: Option::None,
            offset : 0,
            delay : 0,
            shift : 0,
            dc_correction_typ : DCCorrectionType::Continuous,
            slaves: Vec::new(),
            cycle_time: CONTINOUS_TIMELAPS,
            drift_frm_count : STATIC_FRAME_COUNT,
            status : ClockStatus::Registering,
            event,
            typ : MasterSyncType::Cyclic
        }
    }
    pub fn new_with_ptr(master: &'a RawMaster, event_ptr : *const tokio::sync::Notify) -> Self {
       unsafe { Self {
                master,
                ref_p_idx: 0,
                ref_l_idx: Option::None,
                offset : 0,
                delay : 0,
                shift : 0,
                dc_correction_typ : DCCorrectionType::Continuous,
                slaves: Vec::new(),
                cycle_time: CONTINOUS_TIMELAPS,
                drift_frm_count : STATIC_FRAME_COUNT,
                status : ClockStatus::Registering,
                event : &*(event_ptr),
                typ : MasterSyncType::Cyclic
            }
        }
    }

// ==============================    Accessors   ==============================

    /// Configure the periode used when automatic resynchronisation is enable <br>
    /// - time Time in microseconds between each synchronisation control (default value is 1ms)
    /// - drift: frames limits used to trigger a DC synchronization with drift and continueous clock (default value is 15000)
    /// Send "Unauthorized command" error if DC is active.
    pub fn set_sync_loop_limits(&mut self, time: u64, drift : usize) -> Result<(), EthercatError<&'static str>> {
        if self.status != ClockStatus::Active  {
            self.cycle_time = match time == 0 {
                true => CONTINOUS_TIMELAPS,
                _ => time,
            };
            self.drift_frm_count = match drift == 0 {
                true => STATIC_FRAME_COUNT,
                _ => drift
            };
            return Ok(());
        }
        else { return Err(EthercatError::Protocol("DC: Command not authorized with the current instance state")); }
    }

    /// Set control loop parameter for the slave used in the current distributed clock instance.<br>
    /// Only parameter 1 and 3 are available in "Write" mode<br>
    /// This data in specific to the user implementation<br>
    /// - slv_idx: Index of the slave in ethercat loop<br>
    /// - param_1: Parameter 1<br>
    /// - param_3: Parameter 3
    pub async fn set_loop_paramters(&mut self, slv_idx : u16, param_1 : u16, param_3 : u16) {
        for slv in self.slaves.iter_mut() {
            if slv_idx == slv.index {
                slv.clock.control_loop_params[0] = param_1;
                slv.clock.control_loop_params[2] = param_3;
                self.master.fprw(slv.index, dc::rcv_time_loop_0, param_1).await;
                self.master.fprw(slv.index, dc::rcv_time_loop_2, param_3).await;
            }
        }
    }

    /// Get control loop parameter for the slave used in the current distributed clock instance.
    // - slv_idx: Index of the slave in etrercat loop
    pub fn get_loop_parameters(&self, slv_idx : u16) -> Option<[u16;3]> {
        let mut res: Option<[u16;3]> = Option::None;
        for slv in self.slaves.iter() {
            if slv_idx == slv.index {
                res = Some([slv.clock.control_loop_params[0], slv.clock.control_loop_params[1] ,slv.clock.control_loop_params[2]]);
            }
        }
        return res;
    }

    /// Get DC instance status
    pub fn get_status(&self) -> ClockStatus {
        return self.status.clone();
    }

    /// Configure DC for the specific slave. See Isochronous struct for more detail 981 register <br>
    /// - *slave* :  Slave address<br>
    /// - *config* : Slave configuration struct <br>
    pub fn set_dc_slave_config(&mut self, slave : u16, config  : SlaveClockConfigHelper) {
        for slv in self.slaves.iter_mut() {
            if slave != slv.index {
                continue; }

            slv.clock_type = SlaveSyncType::Free;
            if config.sync_0_time != Option::None {
                slv.isochronous.sync.set_generate_sync0(u1::from(true));
                slv.isochronous.interrupt1.set_interrupt(u1::from(true));
                slv.isochronous.sync_0_cycle_time = config.sync_0_time.unwrap();
                slv.clock_type = SlaveSyncType::DcSync0;
            }
            if config.sync_1_time != Option::None {
                slv.isochronous.sync.set_generate_sync1(u1::from(true));
                slv.isochronous.interrupt2.set_interrupt(u1::from(true));
                slv.isochronous.sync_1_cycle_time = config.sync_1_time.unwrap();
                slv.clock_type = SlaveSyncType::DcSync1;
            }
            slv.isochronous.sync.set_enable_cyclic(u1::from(slv.clock_type >= SlaveSyncType::DcSync0));
            slv.isochronous.sync_pulse = config.pulse.unwrap_or_default();
        }
    }

    /// Get a system time in nano seconds
    fn get_local_time(&self) -> u64 {
        return chrono::offset::Local::now().timestamp_nanos().unsigned_abs();
    }

    // Get DC global time in nano seconds
    fn get_global_time(&self) -> u64 {
        if self.get_slave_ref().is_some() {
            return self.get_local_time()
            - self.offset
            - u64::from(self.get_slave_ref().unwrap().clock.system_delay)
            - Duration::from_nanos(100000).as_nanos() as u64; }
        else { return self.get_local_time(); }
    }

    /// Shift time used to synchronoize DC frame send for each slave. Time must be in ns.
    pub fn set_shift(&mut self, shift : u32) {
        self.shift = shift;
    }


// ============================== Other function ==============================

    /// Register on slave to this synchronisation instance
    /// - slv: Slave to register
    /// - index: Index of the slave in the group
    pub fn slave_register(&mut self, idx: u16) -> Result<(), EthercatError<&'static str>> {
        if self.status != ClockStatus::Registering {
            return Err(EthercatError::Slave("Cannot register slaves if clock is initialzed or running")); }
        self.slaves.push(SlaveInfo::new(idx));
        //self.slaves.sort_by(|a, b| a.index.cmp(&b.index));
        return Ok(());
    }

    /// Register many slave to this synchronisation instance<br>
    /// - *slvs*: Slaves to register into a slice. We assume that slaves are sort by croissant order<br>
    /// - *config* : DC basic configuration used for each slave
    /// Return: Number of slave currently register
    pub fn slaves_register(&mut self, slvs : &'a HashMap<u16, Slave>, config : SlaveClockConfigHelper ) -> Result<usize, EthercatError<&'static str>> {
        if self.status != ClockStatus::Registering {
            return Err(EthercatError::Slave("Cannot register slaves if clock is initialzed or running")); }
        let mut i : usize = match self.slaves.is_empty() {
            false => self.slaves.len(),
            _ => 1
        };
        for slv in slvs.iter() {
            self.slaves.push(SlaveInfo::new(*slv.0));
            self.set_dc_slave_config(*slv.0, config.clone());
            i += 1;
        }
        return Ok(i);
    }


    /// Unregister slave if  exist and it's not the reference slave
    /// This function can only be done in init **init** and **register** clock status
    /// - *slave* : Index of the slave to remove (index in EtherCAT loop)
    pub fn slave_unregister(&mut self, slave : u16) {
        if slave == self.ref_p_idx || self.status != ClockStatus::Active {
            return;
        }

        // Search index of the slave in the registers list and remove it
        let mut idx: Option<usize> = Option::None;
        let mut i : usize = 0;
        for slv in self.slaves.iter() {
            if slv.index == slave { idx = Some(i); }
            i += 1;
        }
        if idx.is_some() {
            self.slaves.remove(idx.unwrap());
        }

        //Update reference index in the the vector slaves

    }

    // Store slave index (reguard to the physical loop) in the 'slv_ref_index' attribute
    // Search and store index of the reference slave reguard to vector 'slaves'
    fn search_vector_ref_index(&mut self, phys_index : u16) {
        self.ref_l_idx = Option::None;
        self.ref_p_idx = phys_index;
        let mut lidx : usize = 0;
        while lidx < self.slaves.len() {
            if self.slaves[lidx].index == phys_index {
                self.ref_l_idx = Some(lidx);
            }
            lidx += 1;
        }
    }

    /// Set a slave reference for distributed clock, compute delay and notify all the ethercat loop <br>
    /// *index*: Index of the slave used as reference (must be the first slave of the physical loop)<br>
    /// Warning: This method use dynamic allocation don't used in "production" phase
    /// Return an error:
    ///     - In case of slave reference doesn't register in this DC instance
    ///     - In case of command execution fail
    pub async fn init(&mut self, index : u16) -> Result<(), EthercatError<&'static str>> {
        if usize::from(index) > self.slaves.len() {
            return Err(EthercatError::Slave("Reference doesn't register in this DC instance")); }
        if self.status == ClockStatus::Active {
            return Err(EthercatError::Slave("Cannot initialize a clack that active")); }

        // Store reference index
        self.search_vector_ref_index(index);
        assert_ne!(self.ref_l_idx, Option::None);

        // Check number of slave declared
        let local_time : u64 = self.get_local_time();
        if self.master.brw(dc::rcv_time_brw, 0).await.answers == 0 {
            return Err(EthercatError::Protocol("DC initialisation - No slave found"));
        }

        //Force a reset before offset and delay computation, also disable DC
        self.reset().await;

        //ETG 1020 - 22.2.4 Master Control loop - Initialize loop parameter for all DC node
        self.master.bwr(dc::rcv_time_loop_0, 0x1000u16).await;
        self.master.bwr(dc::rcv_time_loop_2, u16::from_le_bytes([0x04u8, 0x0cu8])).await;
        for slv in self.slaves.iter_mut() {
            slv.clock.control_loop_params[0] = 0x1000u16;
            slv.clock.control_loop_params[2] = u16::from_le_bytes([0x04u8, 0x0cu8]);
        }

        //For each engine read rcv local time, port rcv time and status
        //Get N value of DC
        const ARRAY_MAX_SIZE : usize = 8;
        let mut dc_vec : [Vec<DistributedClock>;ARRAY_MAX_SIZE] = Default::default();
        for i in 0..ARRAY_MAX_SIZE {
            self.slaves.iter_mut().map(|slv| async {
                let dc: PduAnswer<DistributedClock> = self.master.fprd(slv.index, dc::clock).await;
                if dc.answers != 0 {
                    slv.clock = dc.value;
                }
            }).collect::<Vec<_>>().join().await;
            for slv in self.slaves.iter() {
                dc_vec[i].push(slv.clock.clone());
            }
        }
        // Compute mean
        for j in 0..self.slaves.len() {
            let mut rcvt : [u128;4] = [0,0,0,0];
            let (mut st, mut lt) = (0u128,0u128);
            for i in 0..ARRAY_MAX_SIZE {
                for k in 0..4 { rcvt[k] += u128::from(dc_vec[i][j].received_time[k]); }
                st += u128::from(dc_vec[i][j].system_time);
                lt += u128::from(dc_vec[i][j].local_time);
            }
            for k in 0..4 { self.slaves[j].clock.received_time[k] = (rcvt[k] / ARRAY_MAX_SIZE as u128) as u32; }
            self.slaves[j].clock.system_time = (st / ARRAY_MAX_SIZE as u128) as u64;
            self.slaves[j].clock.local_time = (lt / ARRAY_MAX_SIZE as u128) as u64;
        }
        //Reset rcv time for non active port
        self.slaves.iter_mut().map(|slv| async {
            let status : PduAnswer<DLStatus> = self.master.fprd(slv.index, dl::status).await;
            if status.answers != 0 {
                //Check and clean absurd value
                if slv.clock.received_time[3] < slv.clock.received_time[0] || !status.value.port_link_status()[3] { slv.clock.received_time[3] = 0; }
                if slv.clock.received_time[2] < slv.clock.received_time[0] || !status.value.port_link_status()[2] { slv.clock.received_time[2] = 0; }
                if slv.clock.received_time[1] < slv.clock.received_time[0] || !status.value.port_link_status()[1] { slv.clock.received_time[1] = 0; }
            }
        }).collect::<Vec<_>>().join().await;

        // Compute delay between slave (ordered by index)
        self.update_delay().await?;

        // Compute offset between localtime and slave reference local time
        self.update_offset(local_time).await;

        for slv in self.slaves.iter_mut()  {
            slv.clock.control_loop_params[0] = 0x1000u16;
            slv.clock.control_loop_params[2] = u16::from_le_bytes([0x00u8, 0x0cu8]);
        }

        // Send in // transmission delay, offset and isochronous config
        self.slaves.iter().map(|slv| async {
            //Ignore reference and non active slave
            if slv.index >= self.ref_p_idx {
                //Send offset and delay
                let ans = self.master.fpwr(slv.index, dc::rcv_time_delay, slv.clock.system_delay).await;
                assert_eq!(ans.answers, 1);
                let ans = self.master.fpwr(slv.index, dc::rcv_time_offset, slv.clock.system_offset).await;
                assert_eq!(ans.answers, 1);

                // Enable sync unit
                let ans = self.master.fpwr(slv.index, dc::rcv_time_loop_2, slv.clock.control_loop_params[2]).await;
                assert_eq!(ans.answers, 1);
                let ans = self.master.fpwr(slv.index, dc::rcv_time_loop_0, slv.clock.control_loop_params[0]).await;
                assert_eq!(ans.answers, 1);
            }
        }).collect::<Vec<_>>().join().await;

        if cfg!(debug_assertions) {
            for slv in self.slaves.iter() {
                let s : String = String::from("Slave: ") + slv.index.to_string().as_str();
                dbg!(s);
                dbg!(slv.clock.clone());
            }
        }

        //Static drif correction
        for _i in 0..self.drift_frm_count {
            self.master.bwr(dc::rcv_system_time, self.get_global_time()).await;
        }

        self.status = ClockStatus::Initialized;
        return Ok(());
    }

    /// Start dc synchronisation. Use cycle time as execution period<br>
    /// Once the automatic control is start, time cannot period cannot be changed<br>
    /// Start cyclic time correction only with conitnuous drift flag set
    pub async fn sync(&mut self) -> Result<(), EthercatError<&'static str>>{
        if self.status != ClockStatus::Initialized {
            return Err(EthercatError::Protocol("Cannot run DC clock synchro until delay wasn't computed")); }

        self.status = ClockStatus::Active;
        self.typ = MasterSyncType::Dc;
        // //Static drif correction
        // for _i in 0..self.drift_frm_count {
        //     self.master.bwr(dc::rcv_system_time, self.get_global_time()).await;
        // }

        //Start sync
        let t: u64 = self.get_global_time() + Duration::from_nanos(0).as_nanos() as u64;
        self.slaves.iter_mut().map(|slv| async {
            self.master.fpwr(slv.index, isochronous::slave_sync, slv.isochronous.sync).await;
            self.master.fpwr(slv.index, isochronous::slave_start_time, t as u32).await;
            self.master.fpwr(slv.index, isochronous::slave_sync_time_0, slv.isochronous.sync_0_cycle_time).await;
            self.master.fpwr(slv.index, isochronous::slave_sync_time_1, slv.isochronous.sync_1_cycle_time).await;
            self.master.fpwr(slv.index, isochronous::slave_latch_edge_0, slv.isochronous.latch_0_edge).await;
            self.master.fpwr(slv.index, isochronous::slave_latch_edge_1, slv.isochronous.latch_1_edge).await;
        }).collect::<Vec<_>>().join().await;

        assert_eq!(thread_priority::set_current_thread_priority(ThreadPriority::Max).is_ok(), true);

        dbg!(self.cycle_time);

        let mut dbg_lim: i32 = 0;
        let mut interval: time::Interval = time::interval(Duration::from_nanos(self.cycle_time));
        let mut watched_slave_idx : usize = 0;
        while self.status == ClockStatus::Active {
            //Wait tick
            interval.tick().await;

            //Continuous drift correction and survey
            if self.status == ClockStatus::Active && self.dc_correction_typ == DCCorrectionType::Continuous {
                let dt: u64 = self.get_global_time();
                let mut alstatus : u8 = 0;
                //Send data in the same frame
                (
                    async {self.master.bwr(dc::rcv_system_time, dt).await; },
                    // async {
                    //     self.slaves.iter_mut().map(|slv| async {
                    //         let ans = self.master.fprd(slv.index, dc::rcv_time_diff).await;
                    //         assert_eq!(ans.answers, 1);
                    //         slv.clock.system_difference = TimeDifference::new_value(ans.value);
                    //     }).collect::<Vec<_>>().join().await;
                    // },
                    async {
                        let ans = self.master.fprd(self.slaves[watched_slave_idx].index, dc::rcv_time_diff).await;
                        assert_eq!(ans.answers, 1);
                        self.slaves[watched_slave_idx].clock.system_difference = TimeDifference::new_value(ans.value);
                    },
                    async { alstatus = self.master.brd(dls_user::r3).await.value }
                ).join().await;
                _ = self.survey_time_sync().await; //At the moment we don't care about error

                watched_slave_idx += 1;
                watched_slave_idx %= self.slaves.len();

                if alstatus > AlState::PreOperational as u8 {
                    dbg_lim += 1;
                }
            }

            if cfg!(debug_assertions) {
                // Debug tool
                if dbg_lim >= 4000 {
                    self.status = ClockStatus::Initialized;
                }
            }

            //Notify main thread that IO operation can be performed
            self.event.notify_one();
        }
        return Ok(());
    }

    /// Send a reset clock to all slave
    /// If status is not in REGISTER, aslso reset DC+Isochronous clock
    pub async fn reset(&mut self) {
        if self.status == ClockStatus::Active {
            self.status = ClockStatus::Initialized;
        }

        if self.status != ClockStatus::Registering {
            for slv in self.slaves.iter_mut(){
                slv.clock = DistributedClock::new();
                slv.isochronous = Isochronous::new();
            }
        }
        (
            async { self.master.bwr(dc::clock, DistributedClock::new()).await; },
            async { self.master.bwr(isochronous::slave_cfg, Isochronous::new()).await; }
        ).join().await;
    }

    /// Compute for each slave of the current group the system time latency between local clock and reference,
    /// internal prcessing delay and net frame delay transmission.
    /// At the computation purpose,  transmute T and "X" slaves branch to a virtual straight line by trimming
    /// See for much detail
    async fn update_delay(&mut self) -> Result<(), EthercatError<&'static str>> {

        //Compute delay between two successive master/slave - begin from the last slave of the loop
        let mut prv_slave: Option<&SlaveInfo> = Option::None;
        for slv in self.slaves.iter_mut().rev() {
            //Ignore slave before reference
            if slv.index < self.ref_p_idx {
                continue;
            }

            if prv_slave.is_some() && slv.clock.received_time[1] != 0 {
                if prv_slave.unwrap().clock.received_time[3] != 0 {
                    slv.clock.system_delay = ((slv.clock.received_time[1] - slv.clock.received_time[0])
                        - (prv_slave.unwrap().clock.received_time[3] - prv_slave.unwrap().clock.received_time[2])
                        - (prv_slave.unwrap().clock.received_time[2] - prv_slave.unwrap().clock.received_time[1])) / 2;
                } else if prv_slave.unwrap().clock.received_time[2] != 0 {
                    slv.clock.system_delay = ((slv.clock.received_time[1] - slv.clock.received_time[0])
                        - (prv_slave.unwrap().clock.received_time[2] - prv_slave.unwrap().clock.received_time[1])) / 2;
                } else if prv_slave.unwrap().clock.received_time[1] != 0 {
                    slv.clock.system_delay = ( slv.clock.received_time[1] - slv.clock.received_time[0] ) / 2;
                }
            }
            else {
                // No time set -> no dc ?
            }
            prv_slave = Some(slv);
        }

        //Compute transmition latence with the reference + notify slave from delay
        self.delay = self.get_slave_ref().unwrap().clock.system_delay;
        self.slaves[self.ref_l_idx.unwrap()].clock.system_delay = 0;
        for slv in self.slaves.iter_mut() {
            if slv.index != self.ref_p_idx && self.delay > slv.clock.system_delay {
                slv.clock.system_delay = self.delay - slv.clock.system_delay;
            }
        }
        return Ok(());
    }

    /// Apply a correction on DC offset drift<br>
    /// This operation must be done after delay computation
    /// - *local_time*: Local time when BRW 900 was set
    async fn update_offset(&mut self, local_time : u64) {
        //Get and store local ref time
        let ref_time : u64 = self.get_slave_ref().unwrap().clock.local_time;
        self.offset = local_time - ref_time - u64::from(self.delay);

        dbg!(self.offset);
        // Compute offset
        for slv in self.slaves.iter_mut() {
            //Ignore slave before reference
            if slv.clock_type >= SlaveSyncType::DcSync0 {
                slv.clock.system_offset = (i128::from(slv.clock.local_time as u32) - i128::from(ref_time as u32)) as u64;
                //slv.clock.system_offset = (i128::from(slv.clock.received_time[0] as u32) - i128::from(ref_time as u32)) as u64;
            }
        }
    }

    /// Check clock synchronization for each slave
    /// If one clock is divergent
    async fn survey_time_sync(&mut self) -> Result<(), EthercatError<&'static str>> {
        for slv in self.slaves.iter_mut() {
            let buff: TimeDifference = slv.clock.system_difference;
            let is_prv_desync: bool = slv.is_desync;
            slv.is_desync = u32::from(buff.mean().value()) > 500;
            let desync: i32 = u32::from(buff.mean().value()) as i32 * match bool::from(buff.sign()) { true => -1, false => 1 };
            print!("{desync} ");
            //Disable temp
            if slv.is_desync && !is_prv_desync {
            //     return Err(EthercatError::Slave("DC deviation detected for the slave"));
            }
        }
        print!("\n");
        return Ok(());
    }

    //Get slave ref
    fn get_slave_ref(&self) -> Option<&SlaveInfo>{
        if self.ref_l_idx.is_some() {
            return Some(&self.slaves[self.ref_l_idx.unwrap()]);
        }
        return Option::None;
    }
}

/// Implement drop trait for SyncClock
impl Drop for SyncClock<'_> {
    fn drop(&mut self) {
        while self.slaves.len() != 0 {
           self.slaves.remove(self.slaves.len() - 1usize);
           drop(self.event);
        }
    }
}


///// OBSOLETE CODE
// pub fn build_free_clock(cycle_time : Option<u32>) -> SyncMangerFull {
//     let mut sdo: SyncMangerFull = SyncMangerFull ::default();
//     sdo.cycle_time = cycle_time.unwrap_or_default();
//     sdo.supported_sync_type |= 0b1;
//     sdo.min_cycle_time = 1000; // TODO! Given from slave hardware
//     return sdo;
// }

// pub fn build_sm_clock(cycle_time : Option<u32>, shift_time : Option<u32>, min_cycle_time : u32, get_cycle_time : Option<u16>) -> SyncMangerFull {
//     let mut sdo: SyncMangerFull = SyncMangerFull ::default();
//     sdo.sync_type = 0x01;
//     sdo.cycle_time = cycle_time.unwrap_or_default();
//     if shift_time.is_some(){
//         sdo.shift_time = shift_time.unwrap();
//         sdo.sync_type = 0x22;
//     }
//     sdo.supported_sync_type |= 0b10;
//     if get_cycle_time.is_some() {
//         sdo.supported_sync_type |= 0b01000000_00000000;
//         sdo.get_cycle_time = get_cycle_time.unwrap();
//     }
//     return sdo;
// }

// pub fn build_dc_clock(evt_cycle_time : u16, delay_time : u32, c_c_time : u32,
//         cycle_time : Option<u32> ,shift_time : Option<u32>, evt_cnt : Option<u16>, evt_shift_time : Option<u16>,
//         enable_faillure_cnt : Option<bool>, min_cycle : Option<u32>, max_cycle : Option<u32>, sync_typ : SlaveSyncType ) -> SyncMangerFull {
//     let mut sm = SyncMangerFull ::default();
//     sm.sync_type = 0x02u16;
//     sm.cycle_time = cycle_time.unwrap_or_default();

//     let b1: BitField<u16> = data::BitField::new(2,3);
//     let b2: BitField<u16> = data::BitField::new(5,1);
//     let mut array : [u8;2] = sm.supported_sync_type.to_le_bytes();
//     b1.set(&mut array[0..], if sync_typ == SlaveSyncType::DcSync1 { 0x10 } else { 0x01 });
//     b2.set(&mut array[0..], if shift_time.is_none() { 0x00 } else { 0x01 });
//     sm.supported_sync_type = u16::from_le_bytes(array);

//     sm.shift_time = shift_time.unwrap_or_default();
//     if shift_time.is_some() {
//         //check if sync 1 is none
//     }
//     sm.delay_time = delay_time;
//     sm.cycle_time_evt_small = evt_cycle_time;
//     sm.calc_copy_time = c_c_time;
//     sm.sm_evt_cnt = evt_cnt.unwrap_or_default();
//     sm.shift_time_evt_small = evt_shift_time.unwrap_or_default();
//     sm.toggle_failure_cnt = if enable_faillure_cnt.is_some() { 0x01u16 } else { 0x00u16 } ;
//     sm.min_cycle_dist = min_cycle.unwrap_or_default();
//     sm.max_cycle_dist = max_cycle.unwrap_or_default();
//     return  sm;
// }
