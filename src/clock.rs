use crate::{
    master::Master,
    registers::{clock_latch, DistributedClock, TimeDifference},
    BitField, EthercatError, Field, PduAnswer, RawMaster, Slave, SlaveAddress,
};
use chrono;
use core::fmt;
use core::option::Option;
use std::vec::*;

/// Distribution clock - Physical addr:
/// - ReceiveTimePort0 :    0x0900
/// - ReceiveTimePort1 :    0x0904
/// - ReceiveTimePort2 :    0x0908
/// - ReceiveTimePort3 :    0x090C
/// - SystemTime :          0x0910
/// - ReceivingTimePU :     0x0918
/// - ReceivingTimeOffset : 0x0920
/// - ReceivingTimeDelay :  0x0928
/// - ReceivingTimeDiff :   0x092C
/// - ReceivingTimeLoop0 :  0x0930
/// - ReceivingTimeLoop1 :  0x0932
/// - ReceivingTimeLoop2 :  0x0934

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

#[derive(Debug)]
struct SyncClock<'a> {
    slave_ref: usize,
    command_type: ReferenceClock,
    clock_type: SyncType,
    timestamp_size: bool,
    channel: u8,
    slaves: Vec<SlaveInfo<'a>>,
    cycle_time: usize,
    master: &'a RawMaster,
}

#[derive(Debug, PartialEq)]
struct SlaveInfo<'a> {
    slave_ref: &'a Slave<'a>,
    index: usize,
    clock_range: bool,
    is_dc_supported: bool, // 0: 32 bits, 1 : 64bits
    clock: DistributedClock, // system_time : u64,
                           // system_offset : u64,
                           // system_delay : u32,
                           // system_difference : TimeDifference
}

impl<'a> SlaveInfo<'a> {
    fn new(slv: &Slave<'a>, idx: usize) -> Self {
        Self {
            slave_ref: slv,
            index: idx,
            clock_range: true,
            is_dc_supported: true,
            clock: DistributedClock::new(),
        }
    }
}

impl<'a> SyncClock<'a> {
    fn new(master: &RawMaster) -> Self {
        Self {
            slave_ref: 0,
            command_type: ReferenceClock::ARMW,
            clock_type: SyncType::None,
            timestamp_size: false,
            channel: 0,
            cycle_time: 1000,
            slaves: Vec::new(),
            master,
        }
    }

    fn set_cycle_time(&mut self, time: usize) -> Result<(), ClockError> {
        if time < 20 {
            return Err(ClockError::TimeTooShort);
        }
        self.cycle_time = time;
        return Ok(());
    }

    fn get_slave_clock_capability(&self) {
        todo!();
    }

    fn slave_register(&mut self, slv: &Slave, idx: usize) {
        self.slaves.push(SlaveInfo::new(slv, idx));
        self.slaves.sort_by(|a, b| a.index.cmp(&b.index));
    }

    async fn compute_delay(&mut self) -> Result<(), EthercatError<&str>> {
        for slv in self.slaves.iter() {
            //Brodcast delay command
            let brdw: PduAnswer<u32> = unsafe {
                self.master.brw(
                    crate::registers::clock_latch,
                    chrono::offset::Local::now().timestamp() as u32,
                )
            }
            .await;
        }
        //For each engine read rcv time
        for slv in self.slaves.iter_mut() {
            let dc: PduAnswer<DistributedClock> = unsafe {
                self.master
                    .read(slv.slave_ref.get_address(), crate::registers::clock)
            }
            .await;
            if dc.answers == 1 {
                slv.clock = dc.value;
            }
        }

        //Compute delay
        let mut prv_slave: Option<&SlaveInfo<'a>> = Option::None;
        for slv in self.slaves.iter_mut().rev() {
            if prv_slave != Option::None {
                slv.clock.system_delay = ((slv.clock.received_time.port1 as u64
                    - slv.clock.received_time.port0 as u64
                    - prv_slave.unwrap().clock.received_time.port1 as u64
                    - prv_slave.unwrap().clock.received_time.port0 as u64
                    - prv_slave.unwrap().clock.system_offset
                    - prv_slave.unwrap().clock.system_delay as u64)
                    / 2_) as u32;
            }
            prv_slave = Some(slv);
        }

        //Send delay
        for slv in self.slaves.iter() {
            let answer = self
                .master
                .write(
                    slv.slave_ref.get_address(),
                    crate::registers::clock_delay,
                    slv.clock.system_delay,
                )
                .await;
            if answer.answers != 1 {
                return Err(crate::EthercatError::Slave(
                    "Error on distributed clock delay synchronisation",
                ));
            }
        }
        return Ok(());
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
