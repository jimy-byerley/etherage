use crate::{
    registers::{DistributedClock}, EthercatError, PduAnswer, RawMaster, Slave, SlaveAddr
};
use chrono;
use core::option::Option;
use std::{vec::*};
use tokio::{task, time::Duration};

/// Distribution clock - Physical addr:
/// - ReceiveTimePort0 :    0x0900 - 32bits
/// - ReceiveTimePort1 :    0x0904 - 32bits
/// - ReceiveTimePort2 :    0x0908 - 32bits
/// - ReceiveTimePort3 :    0x090C - 32bits
/// - SystemTime :          0x0910 - 64bits
/// - ReceivingTimePU :     0x0918 - 64bits
/// - ReceivingTimeOffset : 0x0920 - 64bits
/// - ReceivingTimeDelay :  0x0928 - 32bits
/// - ReceivingTimeDiff :   0x092C - 32bits
/// - ReceivingTimeLoop0 :  0x0930 - 32bits
/// - ReceivingTimeLoop1 :  0x0932 - 32bits
/// - ReceivingTimeLoop2 :  0x0934 - 32bits

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
enum ReferenceClock {
    #[default]
    ARMW,
    FRMW,
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
    index: usize,
    clock_range: bool,
    is_dc_supported: bool,
    is_desync : bool,
    clock: DistributedClock,
}

impl<'a> SlaveInfo<'a> {
    fn new(slv: &Slave<'a>, idx: usize) -> Self {
        Self {
            slave_ref: slv,
            index: idx,
            clock_range: true,
            is_dc_supported: true,
            is_desync : false,
            clock: DistributedClock::new(),
        }
    }
}

#[derive(Debug)]
struct SyncClock<'a> {
    slave_ref: usize,
    command_type: ReferenceClock,
    clock_type: SyncType,
    timestamp_size: bool,
    auto_resync : bool,
    channel: u8,
    slaves: Vec<SlaveInfo<'a>>,
    cycle_time: usize,
    master: &'a RawMaster,
}

impl<'a> SyncClock<'a> {
    fn new(master: &RawMaster) -> Self {
        Self {
            slave_ref: 0,
            command_type: ReferenceClock::ARMW,
            clock_type: SyncType::None,
            timestamp_size: false,
            auto_resync : false,
            channel: 0,
            cycle_time: 1000,
            slaves: Vec::new(),
            master,
        }
    }

    /// Enable/Disable automatic resynchronisation. Use cycle time as execution period
    /// Once the automatic control is start, time cannot period cannot be changed (TODO: for the moment)
    fn toggle_auto_resync(&mut self) -> bool {
        self.auto_resync = !self.auto_resync;

        if self.auto_resync {
            let task = task::spawn(async {
                let mut interval : time::interval(Duration::from_micros(self.cycle_time));
                while self.auto_resync {
                    self.survey_time_sync().await;
                    interval.tick().await;
                    }
                });
        }

        return self.auto_resync;
    }

    /// Configure the periode used when automatic resynchronisation is enable
    /// - time: Time in microseconds between each synchronisation control (default value set to 1ms)
    fn set_cycle_time(&mut self, time: usize) -> Result<(), ClockError> {
        if time < 20 {
            return Err(ClockError::TimeTooShort);
        }
        self.cycle_time = time;
        return Ok(());
    }

    /// Register on slave to this synchronisation instance
    /// - slv: Slave to register
    /// - index: Index of the slave in the group
    fn slave_register(&mut self, slv: &Slave, idx: usize) {
        self.slaves.push(SlaveInfo::new(slv, idx));
        self.slaves.sort_by(|a, b| a.index.cmp(&b.index));
    }

    /// Register many slave to this synchronisation instance
    /// - slvs: Slaves to register into a slice. We assume that slaves are sort by croissant order
    fn slaves_register(&mut self, slvs : &[Slave] ) {
        let mut i : usize = match self.slaves.is_empty() > 0 {
            false => self.slaves.len(),
            _ => 1
        };
        for slv in slvs {
            self.slaves.push(SlaveInfo::new(slv, i));
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
        let timeref: u64 = self.slaves[self.slave_ref].clock.receive_time_unit;
        let mut prv_slave: Option<&SlaveInfo<'a>> = Option::None;
        for slv in self.slaves.iter_mut().rev() {
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

        //Send computation result to each slave delay
        for slv in self.slaves.iter() {
            let answer = self.master.write(slv.slave_ref.get_address(), crate::registers::clock, slv.clock.clone()).await;
            if answer.answers != 1 {
                return Err(crate::EthercatError::Slave("Error on distributed clock delay synchronisation"));
            }
        }

        return Ok(());
    }

    /// Check clock synchronization for each slave
    /// If one clock is divergent - send a sync request
    async fn survey_time_sync(&mut self) {

        let mut is_resync_needed = false;
        for slv in self.slaves.iter_mut() {
            let answer = unsafe {self.master.fprd(slv.slave_ref.get_index(), register::clock_diff) }.await;
            if answer.answers != 0 {
                return; //TODO throw error
            }
            slv.clock.system_difference = answer.value;
            slv.is_desync = slv.clock.system_difference.mean().value() > 1000;
            if slv.is_desync {  is_resync_needed = true; }
        }
        if is_resync_needed {
            self.resync();
        }
    }

    fn set_reference_slave(&mut self, slv_ref_index : usize) {
        self.slave_ref = slv_ref_index;

        //For all slave, update reference slave and offset time
        for slv in self.slaves.iter() {
            let answer = unsafe {self.master.write(slv.slave_ref.get_address(), crate::registers::clock_delay, slv.slave_ref)}.await;
            if answer.answers != 1 {
                return Err(crate::EthercatError::Slave("Error on distributed clock delay synchronisation"));
            }
        }
    }

    fn resync(&self) {
        for slv in self.slaves {
            //if slv.slave_ref.is_dc_capable() {
                //TODO replace this code with adapted register
                //let delay: Field<u64> = Field::simple(slv.clock.system_difference);
                //let time: [u8; 4] = [0u8; 4];
                //delay.pack(time, slv.clock.system_delay);
                //unsafe { self.master.write(slv.slave_ref.get_address(), offset, time) };
            //}
        }
    }

    /// Set the index of Slave used as clocked reference (must be the first with DC capabalities)
    fn set_reference(&self, idx: usize) -> bool {
        if self.slaves.len() > idx {
            self.slave_ref = idx;
            return true;
        } else {
            return false;
        }
    }
}
