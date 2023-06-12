use crate::PduData;

use { core::fmt, crate::registers::DistributedClock, crate::registers::TimeDifference };

// Isochroneous PDI cloking

// struct Sync_clock{
//     _reserved : char,
//     _sync0 : bool,
//     _sync1 : bool,
//     _sync_pulse : i16,
//     _interrupt1 : bool,
//     _interrupt2 : bool,
//     _cyclicOpStartTm : i32,
//     _latch0PEdge : bool,
//     _latch0NEdge : bool,
//     _latch1PEdge : bool,
//     _latch1NEdge : bool,
//     _latch0PEvt : bool,
//     _latch0NEvt : bool,
//     _latch1PEvt : bool,
//     _latch1NEvt : bool,
//     _latch0 : i16,
//     _latch0 : i16,
//     _latch1 : i16,
//     _latch1 : i16,
// }

#[derive(Default)]
enum ReferenceClock{
    #[default] ARMW,
    FRMW
}

#[derive(Debug, PartialEq)]
struct SyncClock {
    slave_ref : i32,
    command_type: ReferenceClock
}

impl Sync_clock {

}

impl DistributedClock{

    pub fn new() -> Self{
        return Self{
            received_time: [0, 0, 0, 0],
            system_time: 0,
            receive_time_unit: 0,
            system_offset: 0,
            system_delay: 0,
            system_difference,
            reserved: [0, 0, 0],
        };
     }

    pub fn get_sync_pdu(&self) -> PduData{

    }

    // Read the data shift between slave clock and master clock previously send
    pub fn update_delay_shift(&self, PduData) {

    }

    pub fn read(&self, data : &u8[])
    {

    }


}
