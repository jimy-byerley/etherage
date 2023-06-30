use crate::{
    registers::{DistributedClock}, EthercatError, PduAnswer, RawMaster, Slave
};
use chrono;
use core::option::Option;
use std::{vec::*};
use tokio::{task, time::{Duration, self}};

/// Default frame count used to set static drift synchronization
const frm_drift_default : usize = 15000;
/// Default time (in µs) used to trigger system time synchronization
const time_drift_default : usize = 1;

/// Isochroneous PDI cloking struct
struct Isochronous {
    //_byte
    reserved1: u8,
    sync: u8,        // CyclicOperationTime + Sync0 + Sync1 + u5
    sync_pulse: u16, // Optional multiple of 10ns
    reserved10: [u8; 10],
    interrupt1: u8, //Interrupt1Status + u7
    interrupt2: u8, //Interrupt2Status + u7
    cyclic_op_start_time: u32,
    reserved12: [u8; 12],
    sync_0_cycle_time: u32,
    sync_1_cycle_time: u32,
    latch_0_edge: u16, // latch0PosEdge + latch0NegEdge + u14
    latch_1_edge: u16, // latch0PosEdge + latch0NegEdge + u14
    reserved3: [u8; 4],
    latch_0_evt: u8, // latch0PosEdge + latch0NegEdge + u6
    latch_1_evt: u8, // latch0PosEdge + latch0NegEdge + u6
    latch_0_pos_edge_value: u32,
    reserved4: [u8; 4],
    latch_0_neg_edge_value: u32,
    reserved5: [u8; 4],
    latch_1_pos_edge_value: u32,
    reserved6: [u8; 4],
    latch_1_neg_edge_value: u32,
    reserved7: [u8; 4],
}

#[derive(Debug, PartialEq)]
enum ClockError {
    TimeTooShort,
    TimeTooLong,
    UnsupportedDevice,
    UnauthorizedCommand,
}

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

#[derive(Debug)]
struct SyncClock<'a> {
    slave_ref: u16,
    dc_correction_typ : DCCorrectionType,
    clock_type: SyncType,
    timestamp_size: bool,
    is_active : bool,
    channel: u8,
    slaves: Vec<SlaveInfo<'a>>,
    master: &'a RawMaster,
    cycle_time: usize,
    frm_lim : usize,
}

impl<'a> SyncClock<'a> {
    pub fn new(master: &RawMaster) -> Self {
        Self {
            slave_ref: 0,
            dc_correction_typ : DCCorrectionType::Continuous,
            clock_type: SyncType::None,
            timestamp_size: false,
            is_active : false,
            channel: 0,
            slaves: Vec::new(),
            master,
            frm_lim : frm_drift_default,
            cycle_time: time_drift_default,
        }
    }

    /// Enable/Disable dc synchronisation. Use cycle time as execution period
    /// Once the automatic control is start, time cannot period cannot be changed (TODO: for the moment)
    /// Start cyclic time correction only with conitnuous drift flag set
    pub async fn start_sync (&mut self) {
        self.is_active = !self.is_active;

        //Static drif correction
        for i in [0..self.frm_lim] {
            self.sync();
        }

        //Continuous drift correction
        let mut interval: time::Interval = time::interval(Duration::from_micros(self.cycle_time as u64));
        if self.is_active && self.dc_correction_typ == DCCorrectionType::Continuous {
            while self.is_active {
                interval.tick().await;
                self.sync();
            }
        }
    }

    /// Return current limits used to trigger synchronisation
    pub fn get_drift_lim(&self) -> (usize, usize){
        return  (self.frm_lim, self.cycle_time);
    }

    /// Configure the periode used when automatic resynchronisation is enable
    /// Time in microseconds between each synchronisation control (default value set to 1ms)
    /// Default value is set if time < 100 µs
    /// Send "Unauthorized command" error if DC is active.
    pub fn set_cycle_time(&mut self, time: usize) -> Result<(), ClockError> {
        if self.is_active {
            self.cycle_time = match time < 100 {
                true => time_drift_default,
                _ => time,
            };
            return Ok(());
        }
        else { return Err(ClockError::UnauthorizedCommand); }
    }

    /// Set frames limits used to trigger a DC synchronization with drift and continueous clock
    /// Minimum value set at 1000. If value too small, set default value (15000)
    pub fn set_frame_limit(&mut self, lim : usize) {
        if lim >= 1000 {
            self.frm_lim = lim; }
        else {
            self.frm_lim = frm_drift_default;
        }
    }

    /// Register on slave to this synchronisation instance
    /// - slv: Slave to register
    /// - index: Index of the slave in the group
    pub fn slave_register(&mut self, slv: &Slave, idx: u16) {
        self.slaves.push(SlaveInfo::new(slv, idx));
        self.slaves.sort_by(|a, b| a.index.cmp(&b.index));
    }

    /// Register many slave to this synchronisation instance
    /// - slvs: Slaves to register into a slice. We assume that slaves are sort by croissant order
    pub fn slaves_register(&mut self, slvs : &[Slave] ) {
        let mut i : usize = match self.slaves.is_empty() {
            false => self.slaves.len(),
            _ => 1
        };
        for slv in slvs {
            self.slaves.push(SlaveInfo::new(slv,i as u16));
            i += 1;
        }
    }

    /// Compute for each slave of the current group the system time latency between local clock and reference,
    /// internal prcessing delay and net frame delay transmission.
    /// At the computation purpose,  transmute T and "X" slaves branch to a virtual straight line by trimming
    /// See for much detail
    async fn compute_delay(&mut self) -> Result<(), EthercatError<&str>> {
        for slv in self.slaves.iter() {
            //Brodcast delay command
            let brdw: PduAnswer<u32> = unsafe {
                self.master.brw(crate::registers::clock_latch,chrono::offset::Local::now().timestamp() as u32)
            }.await;
        }
        //For each engine read rcv time
        for slv in self.slaves.iter_mut() {
            let dc: PduAnswer<DistributedClock> = unsafe {
                self.master.read(slv.slave_ref.get_address(), crate::registers::clock)
            }.await;
            if dc.answers == 1 {
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
    async fn survey_time_sync(&mut self) {

        let mut is_resync_needed: bool = false;
        for slv in self.slaves.iter_mut() {
            let answer: PduAnswer<u32> = unsafe {self.master.fprd(slv.index, crate::registers::clock_diff) }.await;
            if answer.answers != 0 {
                return; //TODO throw error
            }
            slv.clock.system_difference = crate::registers::TimeDifference { value: answer.value };
            let buff: crate::registers::TimeDifference = slv.clock.system_difference;
            slv.is_desync = u32::from(buff.mean().value()) > 1000;
        }
    }

    /// Design a slave reference for distributed clock, compute delay and notify all the ethercat loop
    /// Return an error:
    ///     - In case of slave reference doesn't register in this DC instance
    ///     - In case of command execution fail
    pub async fn set_reference_slave(&mut self, slv_ref_index : u16) -> Result<(), EthercatError<&str>> {
        if usize::from(slv_ref_index) > self.slaves.len() {
            return Err(crate::EthercatError::Slave("Reference doesn't register in this DC instance")); }
        self.slave_ref = slv_ref_index;

        // Compute delay between slave (ordered by index)
        self.compute_delay();

        // Notify slave from delay and drift from reference slave
        for slv in self.slaves.iter() {
            let answer = self.master.write(slv.slave_ref.get_address(), crate::registers::clock, slv.clock.clone()).await;
            if answer.answers != 1 {
                return Err(crate::EthercatError::Slave("Error on distributed clock delay synchronisation"));
            }
        }

        return Ok(());
    }

    /// Set control loop parameter for the slave used in the current distributed clock instance.
    /// Only parameter 1 and 3 are available in "Write" mode
    /// This data in specific to the user implementation
    /// - slv_idx: Index of the slave in ethercat loop
    /// - param_1: Parameter 1
    /// - param_3: Parameter 3
    pub async fn set_loop_paramters(&mut self, slv_idx : u16, param_1 : u16, param_3 : u16) {
        for slv in self.slaves.iter_mut() {
            if slv_idx == slv.index {
                slv.loop_paramters[0] = param_1;
                slv.loop_paramters[2] = param_3;
                let mut data : [u8;6] = [0;6];
                data[0..2].copy_from_slice(&param_1.to_be_bytes()[0..]);
                data[4..6].copy_from_slice(&param_3.to_be_bytes()[0..]);
                if self.master.fpwr(slv.index, crate::registers::clock_loop, data).await.answers == 0u16{
                    //Todo print error
                }
            }
        }
    }

    // Get control loop parameter for the slave used in the current distributed clock instance.
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

    async fn sync(&self) {
        let answer: PduAnswer<u32> = unsafe {self.master.fprd(self.slave_ref, crate::registers::clock_latch).await };
        if answer.answers != 0 {
                //self.master.armw(self.slaves[self.slave_ref + 1], crate::registers::clock_latch, answer.value).await;
        }
    }
}



// Distribution clock - Physical addr:
// - ReceiveTimePort0 :    0x0900 - 32bits
// - ReceiveTimePort1 :    0x0904 - 32bits
// - ReceiveTimePort2 :    0x0908 - 32bits
// - ReceiveTimePort3 :    0x090C - 32bits
// - SystemTime :          0x0910 - 64bits
// - ReceivingTimePU :     0x0918 - 64bits
// - ReceivingTimeOffset : 0x0920 - 64bits
// - ReceivingTimeDelay :  0x0928 - 32bits
// - ReceivingTimeDiff :   0x092C - 32bits
// - ReceivingTimeLoop0 :  0x0930 - 32bits
// - ReceivingTimeLoop1 :  0x0932 - 32bits
// - ReceivingTimeLoop2 :  0x0934 - 32bits