/// Master normally has two different synchronization modes
/// - Cyclic mode
/// - DC Mode
/// In Cyclic mode the master send the process data frame cyclically. The process data frame may be sent with different cycle times.

use crate::{
    registers::{
        dc::{*, self},
        isochronous::{*, self}, DLStatus, dl
    },
    EthercatError,
    rawmaster::{RawMaster, PduAnswer},
};

use chrono;
use core::option::Option;
use tokio::time::{Duration, self};

/// Default frame count used to set static drift synchronization
const FRM_DRIFT_DEFAULT : usize = 15000;
/// Default time (in µs) used to trigger system time synchronization
const CONTINOUS_DRIFT_DEFAULT : usize = 1000;

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

#[derive(Default, Debug, PartialEq)]
enum SlaveSyncType {
    #[default]
    Free,
    Sm,
    Dc,
}

/// Enumerator to define and implement all synchronization type suppported by the etherCAT protocol
/// ETG1020 table 66
#[derive(Debug, PartialEq, Eq, Default)]
pub enum SlaveSyncDetailType {
    #[default]
    Free,
    Sm2,
    Sm3,
    Sm2Shifted,
    Sm3Shifted,
    Dc,
    DcSync0,
    DcSync0Shifted,
    DcSync1,
    DcShifted,
}
impl SlaveSyncDetailType {
    /// Return the 0x1C32 and 0x1C33 value for the synchronisation mode provide in parameter <br>
    /// If index is not used, value is set to 0 by default <br>
    /// Return array with [0x1C32 SI 01; 0x1C33 SI 01]
    pub const fn get_byte(m : SlaveSyncDetailType) -> [u8;2] {
        return match m {
            SlaveSyncDetailType::Free => [0x00u8, 0x00u8],
            SlaveSyncDetailType::Sm2 => [0x01u8, 0x22u8],
            SlaveSyncDetailType::Sm3 => [0x00u8, 0x01u8],
            SlaveSyncDetailType::Sm2Shifted => [0x01u8, 0x22u8],
            SlaveSyncDetailType::Sm3Shifted => [0x00u8, 0x01u8],
            SlaveSyncDetailType::Dc => [0x02u8, 0x02u8],
            SlaveSyncDetailType::DcSync0 => [0x02u8, 0x02u8],
            SlaveSyncDetailType::DcSync0Shifted => [0x02u8, 0x03u8],
            SlaveSyncDetailType::DcSync1 => [0x03u8, 0x03u8],
            SlaveSyncDetailType::DcShifted => [0x03u8, 0x02u8],
        }
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

impl SlaveInfo {
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
    /// Default implementation: cyclic_op_time = 2000000, sync_0_time = 20000000, sync_1_time = none, pulse : none,
    pub fn default() -> Self {
        Self { sync_0_time: Some(2000000), sync_1_time: Option::None, cyclic_start_time: None, pulse: Some(1) }
    }
}

pub struct SyncClock<'a> {
    mode : SlaveSyncDetailType,
    master: &'a RawMaster,
    slave_ref_index: u16,
    //slave_ref : Option<&'b SlaveInfo>,
    dc_correction_typ : DCCorrectionType,
    slaves: Vec<SlaveInfo>,
    cycle_time: usize,
    drift_frm : usize,
    status : ClockStatus,
}

impl<'a> SyncClock<'a> {

    /// Initialize a new disribution clock
    pub fn new(master: &'a RawMaster) -> Self {
        Self {
            mode : SlaveSyncDetailType::Dc,
            master,
            slave_ref_index: 0,
            //slave_ref : Option::None,
            dc_correction_typ : DCCorrectionType::Continuous,
            slaves: Vec::new(),
            drift_frm : FRM_DRIFT_DEFAULT,
            cycle_time: CONTINOUS_DRIFT_DEFAULT,
            status : ClockStatus::Registering,
        }
    }

// ==============================    Accessors   ==============================

    /// Return current limits used to trigger synchronisation for the drift and cycle time
    pub fn get_sync_loop_limits(&self) -> (usize, usize){
        return  (self.drift_frm, self.cycle_time);
    }

    /// Configure the periode used when automatic resynchronisation is enable <br>
    /// - time Time in microseconds between each synchronisation control (default value is 1ms)
    /// Default value is set if time is 0 µs
    /// - drift: frames limits used to trigger a DC synchronization with drift and continueous clock (default value is 15000)
    /// Default value is set if drift is 0
    /// Send "Unauthorized command" error if DC is active.
    pub fn set_sync_loop_limits(&mut self, time: usize, drift : usize) -> Result<(), EthercatError<&'static str>> {
        if self.status != ClockStatus::Active  {
            self.cycle_time = match time == 0 {
                true => CONTINOUS_DRIFT_DEFAULT,
                _ => time,
            };
            self.drift_frm = match drift == 0 {
                true => FRM_DRIFT_DEFAULT,
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
                slv.isochronous.sync.set_generate_sync0(bilge::prelude::u1::from(true));
                slv.isochronous.interrupt1.set_interrupt(bilge::prelude::u1::from(true));//slv.index != self.slave_ref_index
                slv.isochronous.sync_0_cycle_time = config.sync_0_time.unwrap();
                slv.clock_type = SlaveSyncType::Dc;
            }
            if config.sync_1_time != Option::None {
                slv.isochronous.sync.set_generate_sync1(bilge::prelude::u1::from(true));
                slv.isochronous.interrupt2.set_interrupt(bilge::prelude::u1::from(true));
                slv.isochronous.sync_1_cycle_time = config.sync_1_time.unwrap();
                slv.clock_type = SlaveSyncType::Dc;
            }
            slv.isochronous.sync.set_enable_cyclic(bilge::prelude::u1::from(slv.clock_type == SlaveSyncType::Dc));
            slv.isochronous.sync_pulse = config.pulse.unwrap_or_default();
        }
    }

    /// Get a system time formatted in 64bit or 32bit
    fn get_time(&self) -> u64 {
        return chrono::offset::Local::now().timestamp_nanos().unsigned_abs();
    }

// ============================== Other function ==============================

    /// Register on slave to this synchronisation instance
    /// - slv: Slave to register
    /// - index: Index of the slave in the group
    pub fn slave_register(&mut self, idx: u16) -> Result<(), EthercatError<&'static str>> {
        if self.status != ClockStatus::Registering {
            return Err(crate::EthercatError::Slave("Cannot register slaves if clock is initialzed or running")); }
        self.slaves.push(SlaveInfo::new(idx));
        //self.slaves.sort_by(|a, b| a.index.cmp(&b.index));
        return Ok(());
    }

    /// Register many slave to this synchronisation instance<br>
    /// - *slvs*: Slaves to register into a slice. We assume that slaves are sort by croissant order<br>
    /// - *config* : DC basic configuration used for each slave
    /// Return: Number of slave currently register
    pub fn slaves_register(&mut self, slvs : &[u16], config : SlaveClockConfigHelper ) -> Result<usize, EthercatError<&'static str>> {
        if self.status != ClockStatus::Registering {
            return Err(crate::EthercatError::Slave("Cannot register slaves if clock is initialzed or running")); }
        let mut i : usize = match self.slaves.is_empty() {
            false => self.slaves.len(),
            _ => 1
        };
        for slv in slvs.iter() {
            self.slaves.push(SlaveInfo::new(*slv));
            self.set_dc_slave_config(*slv, config.clone());
            i += 1;
        }
        return Ok(i);
    }

    /// Unregister slave if  exist and it's not the reference slave
    /// This function can only be done in init **init** and **register** clock status
    /// - *slave* : Index of the slave to remove (index in EtherCAT loop)
    pub fn slave_unregister(&mut self, slave : u16) {
        if slave == self.slave_ref_index || self.status != ClockStatus::Active {
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
    }

    /// Set a slave reference for distributed clock, compute delay and notify all the ethercat loop <br>
    /// *slv_ref_index*: Index of the slave used as reference (must be the first slave of the physical loop)<br>
    /// Return an error:
    ///     - In case of slave reference doesn't register in this DC instance
    ///     - In case of command execution fail
    pub async fn init(&mut self, slv_ref_index : u16) -> Result<(), EthercatError<&'static str>> {
        if usize::from(slv_ref_index) > self.slaves.len() {
            return Err(crate::EthercatError::Slave("Reference doesn't register in this DC instance")); }
        if self.status == ClockStatus::Active {
            return Err(crate::EthercatError::Slave("Cannot initialize a clack that active")); }

        self.slave_ref_index = slv_ref_index;
        //self.attr_slv_ref(self.slave_ref_index);

        //Force a reset before offset and delay computation
        self.reset().await;

        //ETG 1020 - 22.2.4 Master Control loop - Initialize loop parameter for all DC node
        //self.master.brw(dc::rcv_time_loop_3, u16::from_be_bytes([0x4u8, 0x0cu8])).await;
        //let ans = self.master.fpwr(self.slave_ref_index, dc::rcv_time_loop_3, u16::from_be_bytes([0x04u8, 0x00u8])).await;
        //assert_eq!(ans.answers, 1);
        //self.master.brw(dc::rcv_time_loop_1, 0x1000u16).await;

        //Brodcast clock command to store rcv_port time
        let local_time : u64 = self.get_time();
        if self.master.brw(dc::rcv_time_brw, 0).await.answers == 0 {
            return Err(EthercatError::Protocol("DC initialisation - No slave found"));
        }

        //For each engine read rcv local time, port rcv time and status
        for slv in self.slaves.iter_mut() {
            let dc: PduAnswer<DistributedClock> = self.master.fprd(slv.index, dc::clock).await;
            let status : PduAnswer<DLStatus> = self.master.fprd(slv.index, dl::status).await;
            if dc.answers != 0 && status.answers != 0 {
                slv.clock = dc.value;

                //Check and clean absurd value
                if slv.clock.received_time[3] < slv.clock.received_time[0] || !status.value.port_link_status()[3] { slv.clock.received_time[3] = 0; }
                if slv.clock.received_time[2] < slv.clock.received_time[0] || !status.value.port_link_status()[2] { slv.clock.received_time[2] = 0; }
                if slv.clock.received_time[1] < slv.clock.received_time[0] || !status.value.port_link_status()[1] { slv.clock.received_time[1] = 0; }
            }
        }

        // Compute delay between slave (ordered by index)
        self.update_delay().await?;

        // Compute offset between localtime and slave reference local time
        self.update_offset(local_time).await;

        // Send slave configuration - Access + sync + other param
        for slv in self.slaves.iter() {
            dbg!(&slv.clock);
            let ans = self.master.fpwr(slv.index, isochronous::slave_cfg, slv.isochronous.clone()).await;
            assert_eq!(ans.answers, 1);
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
        let mut interval: time::Interval = time::interval(Duration::from_micros(self.cycle_time as u64));
        let mut drift_interval: time::Interval = time::interval(Duration::from_micros(20u64));

        //Static drif correction
        println!("Static drif correction");
        for _i in [0..self.drift_frm] {
            drift_interval.tick().await;
            //let t = self.master.fprd(self.slave_ref_index, dc::rcv_system_time).await;
            //assert_eq!(t.answers, 1);
            //self.master.bwr(dc::rcv_system_time, t.value).await;
            self.master.bwr(dc::rcv_system_time, self.get_time() - self.get_slave_ref().unwrap().clock.system_offset).await;
        }

        //Continuous drift correction
        println!("Static drif continous");

        if self.status == ClockStatus::Active && self.dc_correction_typ == DCCorrectionType::Continuous {
            for slv in self.slaves.iter() {
                let t: u32 = (self.get_time() - self.get_slave_ref().unwrap().clock.system_offset) as u32;
                self.master.fpwr(slv.index, dc::rcv_time_loop_0, slv.clock.control_loop_params[0]).await;

                //Start DC
                self.master.fpwr(slv.index, isochronous::slave_start_time, t).await;
            }

            let mut lim = 0;
            while self.status == ClockStatus::Active {
                interval.tick().await;

                //let t = self.master.fprd(self.slave_ref_index, dc::rcv_system_time).await;
                //assert_eq!(t.answers, 1);
                //self.master.bwr(dc::rcv_system_time, t.value).await;
                self.master.bwr(dc::rcv_system_time, self.get_time() - self.get_slave_ref().unwrap().clock.system_offset).await;

                let ans: Result<(), EthercatError<&'static str>> = self.survey_time_sync().await;
                if ans.is_err() {
                    println!("{:?}", ans.err());
                };
                lim += 1;
                if lim >= 4000 {
                    self.status = ClockStatus::Initialized; }
            }
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
        self.master.bwr(dc::clock, DistributedClock::new()).await;
        self.master.bwr(isochronous::slave_cfg, Isochronous::new()).await;
    }

    /// Compute for each slave of the current group the system time latency between local clock and reference,
    /// internal prcessing delay and net frame delay transmission.
    /// At the computation purpose,  transmute T and "X" slaves branch to a virtual straight line by trimming
    /// See for much detail
    async fn update_delay(&mut self) -> Result<(), EthercatError<&'static str>> {

        //Compute delay between two successive master/slave - begin from the last slave of the loop
        let mut prv_slave: Option<&SlaveInfo> = Option::None;
        for slv in self.slaves.iter_mut().rev() {
            if slv.index < self.slave_ref_index {
                // Ignore slave before the reference
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
        let ref_delay : u32 = self.get_slave_ref().unwrap().clock.system_delay;
        for slv in self.slaves.iter_mut() {
            if slv.index != self.slave_ref_index && ref_delay > slv.clock.system_delay {
                slv.clock.system_delay = ref_delay - slv.clock.system_delay;
                if self.master.fpwr(slv.index, dc::clock, slv.clock.clone()).await.answers == 0 {
                    return Err(crate::EthercatError::Slave("Cannot send delay and time out offset to slave}"));
                }
            }
        }




        return Ok(());
    }

    /// Apply a correction on DC offset drift<br>
    /// This operation must be done after delay computation
    /// - *local_time*: Local time when BRW 900 was set
    async fn update_offset(&mut self, local_time : u64) {
        let zero : IsochronousSync = Default::default();

        //Get ref time
        let ref_time : u64 = self.get_slave_ref().unwrap().clock.system_time;
        let delay_ref : u64 = u64::from(self.get_slave_ref().unwrap().clock.system_delay);
        self.get_slave_mut_ref().unwrap().clock.system_offset = local_time - ref_time - u64::from(delay_ref);

        // Compute offset
        for slv in self.slaves.iter_mut() {
            // Disable sync unit & apply offset
            self.master.fpwr(slv.index, crate::registers::isochronous::slave_sync, zero.clone()).await;
            if slv.index != self.slave_ref_index && slv.clock_type == SlaveSyncType::Dc {
                slv.clock.system_offset = if ref_time > slv.clock.system_time - u64::from(slv.clock.system_delay)
                    { ref_time - slv.clock.system_time - u64::from(slv.clock.system_delay) } else
                    { slv.clock.system_time - u64::from(slv.clock.system_delay) - ref_time };
                slv.clock.control_loop_params[0] = 0x1000;
                slv.clock.control_loop_params[2] = u16::from_be_bytes([0x04u8, 0x0cu8]);

                self.master.fpwr(slv.index, dc::rcv_time_offset, slv.clock.system_offset).await;
                self.master.fpwr(slv.index, isochronous::slave_start_time, slv.isochronous.cyclic_op_start_time).await;

            } else if slv.index == self.slave_ref_index {
                slv.clock.control_loop_params[0] = 0x1000;
                slv.clock.control_loop_params[2] = u16::from_be_bytes([0x04u8, 0x00u8]);
            }
            // Enable sync unit
            self.master.fpwr(slv.index, dc::rcv_time_loop_0, slv.clock.control_loop_params[0]).await;
            self.master.fpwr(slv.index, dc::rcv_time_loop_2, slv.clock.control_loop_params[2]).await;
            self.master.fpwr(slv.index, isochronous::slave_sync, slv.isochronous.sync.clone()).await;
        }
    }

    /// Check clock synchronization for each slave
    /// If one clock is divergent
    async fn survey_time_sync(&mut self) -> Result<(), EthercatError<&'static str>> {
        for slv in self.slaves.iter_mut() {
            let ans: PduAnswer<u32> = self.master.fprd(slv.index, dc::rcv_time_diff).await;
            if ans.answers == 0  {
                //_ = self.update_offset(local_time);
                return Err(EthercatError::Protocol("Survey time - Data reception fail"));
            }

            slv.clock.system_difference = TimeDifference::new_value(ans.value);
            let buff: TimeDifference = slv.clock.system_difference;
            let is_prv_desync: bool = slv.is_desync;
            slv.is_desync = u32::from(buff.mean().value()) > 500;
            if slv.index == 2 {
                let desync = u32::from(buff.mean().value()) as i32 * match bool::from(buff.sign()) { true => -1, false => 1 };
                println!("{desync}"); }
            //Disable temp
            // if slv.is_desync && !is_prv_desync {
            //     return Err(crate::EthercatError::Slave("DC deviation detected for the slave"));
            // }
        }
        return Ok(());
    }

    //Get slave ref
    fn get_slave_ref(&self) -> Option<&SlaveInfo>{
        for slv in self.slaves.iter() {
            if slv.index == self.slave_ref_index {
                return Some(&slv);
            }
        }
        return Option::None;
    }

    fn get_slave_mut_ref(&mut self) -> Option<&mut SlaveInfo>{
        for slv in self.slaves.iter_mut() {
            if slv.index == self.slave_ref_index {
                return Some(slv);
            }
        }
        return Option::None;
    }

}

/// ETG 1020 table 86 - 0x1C32
struct OutputSM {
    sync_type : u16,
    cycle_time : u32,
    shift_time : u32,
    supported_sync_type : u16,
    min_cycle_time : u32,
    calc_copy_time : u32,
    min_delay_time : u32,
    get_cycle_time : u16,
    delay_time : u32,
    sync0_cycle_time : u32,
    sm_evt_cnt : u16,
    cycle_time_evt_small : u16,
    shift_time_evt_small : u16,
    toggle_failure_cnt : u16,
    min_cycle_dist : u32,
    max_cycle_dist : u32,
    min_sm_sync_dist : u32,
    max_sm_sync_dist : u32,
    //Reserver index 19-31
    sync_error : bool
}

/// ETG 1020 table 87 - 0x1C32
struct InputSM {
    sync_type : u16,
    cycle_time : u32,
    shift_time : u32,
    supported_sync_type : u16,
    min_cycle_time : u32,
    calc_copy_time : u32,
    reserved7 :  u32,
    get_cycle_time : u16,
    delay_time : u32,
    sync0_cycle_time : u32,
    sm_evt_cnt : u16,
    cycle_time_evt_small : u16,
    shift_time_evt_small : u16,
    reserved14 : u16,
    //Reserver index 15-31
    sync_error : bool
}
