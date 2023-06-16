use std::vec::*;
use core::option::Option;
use core::fmt;
use crate::{ registers::{ DistributedClock, TimeDifference}, master::Master, SlaveAddress, RawMaster, Field, BitField };

/// Isochroneous PDI cloking struct
struct Isochronous {
    //_byte
    reserved1 : u8,
    sync : u8,             // CyclicOperationTime + Sync0 + Sync1 + u5
    sync_pulse : u16,      // Optional multiple of 10ns
    reserved10 : [u8;10],
    interrupt1 : u8,       //Interrupt1Status + u7
    interrupt2 : u8,       //Interrupt2Status + u7
    cyclic_op_start_time : u32,
    reserved12 : [u8;12],
    sync_0_cycle_time: u32,
    sync_1_cycle_time: u32,
    latch_0_edge : u16,      // latch0PosEdge + latch0NegEdge + u14
    latch_1_edge : u16,      // latch0PosEdge + latch0NegEdge + u14
    reserved3  : [u8;4],
    latch_0_evt :  u8,       // latch0PosEdge + latch0NegEdge + u6
    latch_1_evt :  u8,       // latch0PosEdge + latch0NegEdge + u6
    latch_0_pos_edge_value : u32,
    reserved4 : [u8;4],
    latch_0_neg_edge_value : u32,
    reserved5 : [u8;4],
    latch_1_pos_edge_value : u32,
    reserved6 : [u8;4],
    latch_1_neg_edge_value : u32,
    reserved7 : [u8;4],
}

struct DelayCmd{
    address : u16,
    data_area : u16,
    port0 : u16,
    port1 : u16,
}

#[derive(Debug, PartialEq)]
enum ClockError {
    TimeTooShort,
    TimeTooLong,
    UnsupportedDevice,
    UnauthorizedCommand,
}

#[derive(Default, Debug, PartialEq)]
enum ReferenceClock{
    #[default] ARMW,
    FRMW
}

#[derive(Default, Debug, PartialEq)]
enum SyncType {
    #[default] None,
    SM,
    DC,
    DCCSM,
}

//TODO temp implementation
#[derive(Default, Debug, PartialEq, Eq)]
struct Slave<'a> {
    master : &'a RawMaster
 }

impl<'a> Slave<'a> {
    fn is_DC_Capable(&self) -> bool { return false; }
    fn get_address(&self) -> SlaveAddress { return SlaveAddress::Logical; }
}

#[derive(Debug, PartialEq)]
struct SyncClock<'a> {
    slave_ref : usize,
    command_type: ReferenceClock,
    clock_type : SyncType,
    timestamp_size : bool,
    channel : u8,
    slaves : Vec<SlaveInfo<'a>>,
    cycle_time : usize,
    master : &'a Master
}

#[derive(Default, Debug, PartialEq)]
struct SlaveInfo<'a>{
    slave_ref : &Slave<'a>,
    index : usize,
    delay : usize,
    distributed_clock : DistributedClock,
}


#[derive(Default, Debug, PartialEq)]
impl<'a> SlaveInfo<'a> {
    fn new(slv : &Slave<'a>, idx : usize) -> Self{
        Self { slave_ref: slv, index: idx, delay: 0, distributed_clock: () }
    }
}


impl<'a> SyncClock<'a> {

    fn new(master : &Master) -> Self{
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

    fn set_cycle_time(&mut self, time : usize) -> Result<(),ClockError>{
        if time < 10 { return Err(ClockError::TimeTooShort); }
        if self.master.s > preo { return Err(ClockError::UnauthorizedCommand); }
        self.cycle_time = time;
        return Ok(());
    }

    fn get_slave_clock_capability(&self) {
        todo!();
    }

    fn slave_register(&mut self, slv : &Slave, idx : usize) {
        self.slaves.push(SlaveInfo::new(slv, idx));
        self.slaves.sort_by(|a, b| { a.index < b.index });
    }

    async fn get_delay(&mut self)
    {
        for slv in self.slaves {
            let mut data : [u8;60] = [u8;60];
            let data = self.master.get_raw().brw(clock_latch, data).await?;
        }
    }

    fn resync(&self) {
        for slv in self.slaves {
            if slv.slave_ref.is_DC_Capable() {
                //TODO replace this code with adapted register
                let delay : Field<usize> = Field::simple(slv.distributed_clock.system_difference);
                let time : [u8;4] = [u8;4];
                delay.pack(time, slv.delay);
                unsafe { self.master.get_raw().write(slv.slave_ref.get_address(), offset, time ) };
            }
        }
    }

    /// Set the index of Slave used as clocked reference (must be the first with DC capabalities)
    fn set_reference(&self, idx : usize) -> bool {
        if self.slaves.len() > idx {
            self.slave_ref = idx;
            return true;
        } else {
            return  false;
        }
    }

}
