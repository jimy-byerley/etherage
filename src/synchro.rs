use crate::{
    registers::{DistributedClock, Isochronous}, EthercatError, PduAnswer, RawMaster, Slave
};
use chrono;
use core::option::Option;
use std::{vec::*};
use tokio::{time::{Duration, self}};

/// Default frame count used to set static drift synchronization
const frm_drift_default : usize = 15000;
/// Default time (in µs) used to trigger system time synchronization
const time_drift_default : usize = 1;

#[derive(Default, Debug, PartialEq)]
enum DCCorrectionType {
    #[default]
    Static,
    Continuous,
}

#[derive(Default, Debug, PartialEq)]
enum SyncType {
    #[default]
    None,
    SM,
    DC,
    DCCSM,
}

#[derive(Debug, PartialEq)]
struct SlaveInfo<'a> {
    slave_ref: &'a Slave<'a>,
    index: u16,
    clock_range: bool,
    is_dc_supported: bool,
    is_desync : bool,
    clock: DistributedClock,
    loop_paramters : [u16;3]
}

impl<'a> SlaveInfo<'a> {
    fn new(slv: &Slave<'a>, idx: u16) -> Self {
        Self {
            slave_ref: slv,
            index: idx,
            clock_range: true,
            is_dc_supported: true,
            is_desync : false,
            clock: DistributedClock::new(),
            loop_paramters : [0;3]
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default)]
enum ClockStatus {
    #[default]
    REGISTER,
    INIT,
    RUN
}

#[derive(Debug)]
struct SyncClock<'a> {
    slave_ref: u16,
    dc_correction_typ : DCCorrectionType,
    clock_type: SyncType,
    timestamp_size: bool,
    slaves: Vec<SlaveInfo<'a>>,
    master: &'a RawMaster,
    cycle_time: usize,
    drift_frm : usize,
    status : ClockStatus,
    activation : u8,
}

impl<'a> SyncClock<'a> {

    /// Initialize a new disribution clock
    pub fn new(master: &RawMaster) -> Self {
        Self {
            slave_ref: 0,
            dc_correction_typ : DCCorrectionType::Continuous,
            clock_type: SyncType::None,
            timestamp_size: false,
            slaves: Vec::new(),
            master,
            drift_frm : frm_drift_default,
            cycle_time: time_drift_default,
            status : ClockStatus::REGISTER,
            activation :0,
        }
    }

// ==============================    Accessors   ==============================

    pub fn set_unit_control(&self, clock_config : Isochronous) {
        for slv in self.slaves.iter() {
            //DC unit control
            self.master.fpwr(slv.index, crate::registers::PduDC::dc_control_unit, clock_config.reserved1);

            //DC activation
            self.master.fpwr(slv.index, crate::registers::PduDC::dc_activation, clock_config.sync);

            //DC latch
            self.master.fpwr(slv.index, crate::registers::PduDC::dc_latch0, clock_config.latch_0_edge);
            self.master.fpwr(slv.index, crate::registers::PduDC::dc_latch1, clock_config.latch_1_edge);
        }
    }

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
    pub fn set_sync_loop_limits(&mut self, time: usize, drift : usize) -> Result<(), EthercatError<&str>> {
        if self.status != ClockStatus::RUN  {
            self.cycle_time = match time == 0 {
                true => time_drift_default,
                _ => time,
            };
            self.drift_frm = match drift == 0 {
                true => frm_drift_default,
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
                slv.loop_paramters[0] = param_1;
                slv.loop_paramters[2] = param_3;
                let mut data : [u8;6] = [0;6];
                data[0..2].copy_from_slice(&param_1.to_be_bytes()[0..]);
                data[4..6].copy_from_slice(&param_3.to_be_bytes()[0..]);
                if self.master.fpwr(slv.index, crate::registers::PduDC::clock_loop, data).await.answers == 0 {
                    //Todo print error
                }
            }
        }
    }

    /// Get control loop parameter for the slave used in the current distributed clock instance.
    // - slv_idx: Index of the slave in etrercat loop
    pub fn get_loop_parameters(&self, slv_idx : u16) -> Option<[u16;3]> {
        let mut res: Option<[u16;3]> = Option::None;
        for slv in self.slaves.iter() {
            if slv_idx == slv.index {
                res = Some(slv.loop_paramters.clone());
            }
        }
        return res;
    }

    /// Get DC instance status
    pub fn get_status(&self) -> ClockStatus {
        return self.status;
    }

    /// Get a system time formatted in 64bit or 32bit
    fn get_time(&self) -> u64 {

        return chrono::offset::Local::now().timestamp().unsigned_abs();

        // let f: crate::BitField<u8> = crate::BitField::<u8>::new(6, 1);
        // let v: bool = f.get(&self.activation.to_be_bytes()[0..]) == 1;

        // return match v {
        //     true => {
        //         let mut array = chrono::offset::Utc::now().timestamp().to_be_bytes();
        //         array[0..4].fill(0);
        //         u64::from_be_bytes(array)
        //     },
        //     _ => chrono::offset::Local::now().timestamp().unsigned_abs(),
        // };
    }

// ============================== Other function ==============================

    /// Register on slave to this synchronisation instance
    /// - slv: Slave to register
    /// - index: Index of the slave in the group
    pub fn slave_register(&mut self, slv: &Slave, idx: u16) -> Result<(), EthercatError<&str>> {
        if self.status != ClockStatus::REGISTER {
            return Err(crate::EthercatError::Slave("Cannot register slaves if clock is initialzed or running")); }
        self.slaves.push(SlaveInfo::new(slv, idx));
        self.slaves.sort_by(|a, b| a.index.cmp(&b.index));
        return Ok(());
    }

    /// Register many slave to this synchronisation instance
    /// - slvs: Slaves to register into a slice. We assume that slaves are sort by croissant order
    pub fn slaves_register(&mut self, slvs : &[Slave] ) -> Result<(), EthercatError<&str>> {
        if self.status != ClockStatus::REGISTER {
            return Err(crate::EthercatError::Slave("Cannot register slaves if clock is initialzed or running")); }
        let mut i : usize = match self.slaves.is_empty() {
            false => self.slaves.len(),
            _ => 1
        };
        for slv in slvs {
            self.slaves.push(SlaveInfo::new(slv,i as u16));
            i += 1;
        }
        return Ok(());
    }

    /// Design a slave reference for distributed clock, compute delay and notify all the ethercat loop <br>
    /// *slv_ref_index*: Index of the slave used as reference (must be the first slave of the physical loop)<br>
    /// Return an error:
    ///     - In case of slave reference doesn't register in this DC instance
    ///     - In case of command execution fail
    pub async fn init(&mut self, slv_ref_index : u16) -> Result<(), EthercatError<&str>> {
        if usize::from(slv_ref_index) > self.slaves.len() {
            return Err(crate::EthercatError::Slave("Reference doesn't register in this DC instance")); }
        if self.status == ClockStatus::RUN {
            return Err(crate::EthercatError::Slave("Cannot initialize a clack that active")); }

        self.slave_ref = slv_ref_index;

        // Compute delay between slave (ordered by index)
        self.compute_delay();

        // Notify slave from delay and drift from reference slave
        for slv in self.slaves.iter() {
            let ans: PduAnswer<()> = self.master.fpwr(slv.index, crate::registers::PduDC::clock, slv.clock.clone()).await;
            if ans.answers == 0 {
                return Err(crate::EthercatError::Slave("Cannot send dely and time out offset to slave}"));
            }
        }

        self.status = ClockStatus::INIT;

        return Ok(());
    }

    /// Start dc synchronisation. Use cycle time as execution period<br>
    /// Once the automatic control is start, time cannot period cannot be changed<br>
    /// Start cyclic time correction only with conitnuous drift flag set
    pub async fn sync(&mut self) -> Result<(), EthercatError<&str>>{
        if self.status != ClockStatus::INIT {
            return Err(EthercatError::Protocol("Cannot run DC clock synchro until delay wasn't computed")); }

        self.status = ClockStatus::RUN;

        //Static drif correction
        for i in [0..self.drift_frm] {
            if self.master.bwr(crate::registers::PduDC::system_clock, self.get_time() ).await.answers == 0 {
                //TODO print error
            }
        }

        //Continuous drift correction
        let mut interval: time::Interval = time::interval(Duration::from_micros(self.cycle_time as u64));
        if self.status == ClockStatus::RUN && self.dc_correction_typ == DCCorrectionType::Continuous {
            for slv in self.slaves.iter() {
                self.master.fpwr(slv.index, crate::registers::PduDC::clock_speed, slv.loop_paramters[0]);
            }

            while self.status == ClockStatus::RUN {
                interval.tick().await;
                if self.master.bwr(crate::registers::PduDC::system_clock, self.get_time() ).await.answers == 0 {
                    //TODO print error
                }

                self.survey_time_sync();
            }
        }

        return Ok(());
    }

    /// Send a reset clock to all slave
    pub async fn reset(&mut self) {
        if self.status == ClockStatus::RUN {
            self.status = ClockStatus::INIT;
        }
        else { return; }

        for slv in self.slaves.iter_mut(){
            slv.clock = DistributedClock::new();
        }
        if self.master.bwr(crate::registers::PduDC::clock, DistributedClock::new()).await.answers == 0 {
            //TODO send error
        };
    }

    /// Compute for each slave of the current group the system time latency between local clock and reference,
    /// internal prcessing delay and net frame delay transmission.
    /// At the computation purpose,  transmute T and "X" slaves branch to a virtual straight line by trimming
    /// See for much detail
    async fn compute_delay(&mut self) -> Result<(), EthercatError<&str>> {
        //Brodcast clock command to store rcv_port time
        if self.master.brw(crate::registers::PduDC::clock_brw,self.get_time() as u32).await.answers == 0 {
            return Err(EthercatError::Protocol("DC initialisation - No slave found"));
        }

        //For each engine read rcv time
        for slv in self.slaves.iter_mut() {
            let dc: PduAnswer<DistributedClock> = self.master.fprd(slv.index, crate::registers::PduDC::clock).await;
            if dc.answers != 0 {
                slv.clock = dc.value;
            }
        }

        //Compute delay - begin from the last slave of the loop
        let timeref: u64 = self.slaves[usize::from(self.slave_ref)].clock.receive_time_unit;
        let mut prv_slave: Option<&SlaveInfo<'a>> = Option::None;
        for slv in self.slaves.iter_mut().rev() {
            if slv.index < self.slave_ref {
                // Ignore slave before the reference
                continue;
            }

            if prv_slave != Option::None {
                if prv_slave.unwrap().clock.received_time[3] != 0 {
                    slv.clock.system_delay = ((slv.clock.received_time[1] - slv.clock.received_time[0])
                        - (prv_slave.unwrap().clock.received_time[3] - prv_slave.unwrap().clock.received_time[2])
                        - (prv_slave.unwrap().clock.received_time[2] - prv_slave.unwrap().clock.received_time[1])) / 2;
                } else if prv_slave.unwrap().clock.received_time[2] != 0 {
                    slv.clock.system_delay = ((slv.clock.received_time[1] - slv.clock.received_time[0])
                        - (prv_slave.unwrap().clock.received_time[2] - prv_slave.unwrap().clock.received_time[1])) / 2;
                } else {
                    slv.clock.system_delay = ( slv.clock.received_time[1] - slv.clock.received_time[0] ) / 2;
                    slv.clock.system_offset = timeref - slv.clock.receive_time_unit;
                }
            }
            prv_slave = Some(slv);
        }

        return Ok(());
    }

    /// Check clock synchronization for each slave
    /// If one clock is divergent
    async fn survey_time_sync(&mut self) -> Result<(), EthercatError<&str>> {
        for slv in self.slaves.iter_mut() {
            let ans: PduAnswer<u32> = self.master.fprd(slv.index, crate::registers::PduDC::rcv_clock_diff).await;
            if ans.answers != 0 {
                //(format!("DC deviation detected for the slave {}", slv.index)).as_str();
                return Err(crate::EthercatError::Slave("DC deviation detected for the slave {}"));
            }

            slv.clock.system_difference = crate::registers::TimeDifference { value: ans.value };
            let buff: crate::registers::TimeDifference = slv.clock.system_difference;
            slv.is_desync = u32::from(buff.mean().value()) > 1000;
        }
        return Ok(());
    }

}
