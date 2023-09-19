/*!
    Master normally has two different synchronization modes
    
    - Cyclic mode
    - DC Mode

    In Cyclic mode the master send the process data frame cyclically. The process data frame may be sent with different cycle times.
    
    TODO
*/

use crate::{
    registers::{
        self,
        AlState,
        dc::DistributedClock,
        isochronous::Isochronous,
        },
    sdo::{self, SyncMangerFull},
    rawmaster::{RawMaster, PduCommand, SlaveAddress}, 
    EthercatError, Slave, Sdo,
    };
use std::{
    time::{Instant, Duration},
    sync::Arc,
    };

use bilge::prelude::*;
use futures_concurrency::future::Join;
use thread_priority::*;



/// Default frame count used to set static drift synchronization, recommented by ETG.1020
const STATIC_FRAME_COUNT: usize = 15000;
/// Default time (in µs) used to trigger system time synchronization (ns)
const CONTINOUS_TIMELAPS: u64 = 2000000;




/**
    implementation of the Distributed Clock (DC) at the master level.
    
    Other kinds of clock do not concern this struct.
*/
pub struct SyncClock {
    // Raw master reference
    master: Arc<RawMaster>,
    /// start instant used as master's clock
    start: Instant,
    
    /// Time shift between master local time and reference local time (this is the difference between the clocks for the same instant, without transmission delay)
    offset: i128,
    /// Delay transmision between master and reference
    delay: u32,
//     // Shift delay used to advance transmission frame and allow "input data" to be available at sm3 event
//     shift: u32,
    
    /// Type of DC used
    dc_correction_typ: DCCorrectionType,
    /// Array of registerd slave
    slaves: Vec<SlaveInfo>,
    /// DC control cycle time
    cycle_time: u64,
    /// Number of frame used to prevent static drift
    drift_frm_count: usize,
    /// Current status of the SyncClock structure
    status: ClockStatus,
    /// Current master synchronization type
    typ: MasterSyncType,
}
/// struct caching per-slave variables for clock synchronization
struct SlaveInfo {
    address: u16,
    clock: registers::dc::DistributedClock,
    isochronous: registers::isochronous::Isochronous,
    clock_type: SlaveSyncType,
}

impl SyncClock {
    /** 
        Initialize a new distributed clock
        
        `slaves` list the slaves to synchronize with this clock. They must support the distributed clock mode and shall be listed in topological order (See [SlaveAddress::AutoIncremented] for topological details)
    */
    pub async fn new(
            master: Arc<RawMaster>, 
            slaves: Vec<u16>, 
            period: Option<Duration>, 
            static_frames: Option<usize>,
            ) -> Result<SyncClock, EthercatError> {
        assert!(slaves.len() != 0, "unable to run clock with no slave");
        
        let mut new = Self {
            master,
            start: Instant::now(),
            offset: 0,
            delay: 0,
//             shift: 0,
            dc_correction_typ: DCCorrectionType::Continuous,
            slaves: Vec::new(),
            cycle_time: period
                            .map(|t| t.as_nanos() as u64)
                            .unwrap_or(CONTINOUS_TIMELAPS),
            drift_frm_count: static_frames.unwrap_or(STATIC_FRAME_COUNT),
            status: ClockStatus::Registering,
            typ: MasterSyncType::Cyclic,
        };
        
        let config = SlaveClockConfigHelper::default();
        for slave in slaves {
            new.slaves.push(SlaveInfo::new(slave));
            new.configure_slave(slave, config.clone());
        }
        new.init().await?;
        
        Ok(new)
    }
    
    /**
        Initialize the distributed clock with all slaves found supporting it.
    */
    pub async fn all(
            master: Arc<RawMaster>, 
            period: Option<Duration>, 
            static_frames: Option<usize>,
            ) -> Result<SyncClock, EthercatError> {
        
        let slaves = master.brd(registers::dl::information).await;
        if slaves.answers == 0 || ! slaves.value.dc_supported()
            {return Err(EthercatError::Master("no slave supporting clock"))}
        
        let raw = &master;
        let slaves = (0 .. slaves.answers).map(|slave|  async move {
                let (slave, info) = (
                    raw.aprd(slave, registers::address::fixed), 
                    raw.aprd(slave, registers::dl::information),
                    ).join().await;
                (slave.one(), info.one())
            })
            .collect::<Vec<_>>().join().await
            .iter()
            .filter(|(_, info)|  info.dc_supported())
            .map(|(slave, _)| slave.clone())
            .collect::<Vec<u16>>();
        
        Self::new(master, slaves, period, static_frames).await
    }
    
    /// return the ethercat master this clock is working with
    pub unsafe fn raw_master(&self) -> &Arc<RawMaster>  {&self.master}
    
    /**
        Set a slave reference for distributed clock, compute delay and notify all the ethercat loop
        
        *index*: Index of the slave used as reference (must be the first slave of the physical loop)
        
        Warning: This method uses dynamic allocation don't use it in realtime operations
        
        Return an error:
            - In case of slave reference doesn't register in this DC instance
            - In case of command execution fail
    */
    async fn init(&mut self) -> Result<(), EthercatError> {
        println!("init");
        if self.status == ClockStatus::Active 
            {return Err(EthercatError::Master("Cannot initialize a clock that active"))}

        // Check number of slave declared
        let local_time = self.local_time();
        if self.master.brw(registers::dc::rcv_time_brw, 0).await.answers == 0 
            {return Err(EthercatError::Master("DC initialisation - No slave found"))}

        // Force a reset before offset and delay computation, also disable DC
        self.reset().await;

        // ETG.1020 - 22.2.4 Master Control loop - Initialize loop parameter for all DC node
        // these hardcoded constants are from the specs
        self.master.bwr(registers::dc::rcv_time_loop_2, u16::from_le_bytes([4, 12])).await;
        self.master.bwr(registers::dc::rcv_time_loop_0, 0x1000u16).await;
        for slv in self.slaves.iter_mut() {
            slv.clock.control_loop_params[0] = 0x1000u16;
            slv.clock.control_loop_params[2] = u16::from_le_bytes([4, 12]);
        }

        // For each engine read rcv local time, port rcv time and status
        // Collect many values of DC
        const ARRAY_MAX_SIZE: usize = 8;
        let mut dc_vec: [Vec<DistributedClock>; ARRAY_MAX_SIZE] = Default::default();
        for i in 0..ARRAY_MAX_SIZE {
            self.slaves.iter_mut().map(|slv| async {
                let dc = self.master.fprd(slv.address, registers::dc::clock).await;
                if dc.answers != 0 {
                    slv.clock = dc.value;
                }
            }).collect::<Vec<_>>().join().await;
            for slv in self.slaves.iter() {
                dc_vec[i].push(slv.clock.clone());
            }
        }
        // Compute mean
        // TODO: use compensated sums for better precision
        const PORTS: usize = 4;
        for j in 0..self.slaves.len() {
            let mut rcvt = [0; PORTS];
            let (mut st, mut lt) = (0u128, 0u128);
            for i in 0..ARRAY_MAX_SIZE {
                for k in 0 .. rcvt.len() { rcvt[k] += u128::from(dc_vec[i][j].received_time[k]); }
                st += u128::from(dc_vec[i][j].system_time);
                lt += u128::from(dc_vec[i][j].local_time);
            }
            for k in 0 .. rcvt.len() { self.slaves[j].clock.received_time[k] = (rcvt[k] / ARRAY_MAX_SIZE as u128) as u32; }
            self.slaves[j].clock.system_time = (st / ARRAY_MAX_SIZE as u128) as u64;
            self.slaves[j].clock.local_time = (lt / ARRAY_MAX_SIZE as u128) as u64;
        }
        // Reset rcv time for non active port
        self.slaves.iter_mut().map(|slv| async {
            let status = self.master.fprd(slv.address, registers::dl::status).await;
            if status.answers != 0 {
                // Check and clean absurd value
                for i in 1 .. PORTS {
                    if slv.clock.received_time[i] < slv.clock.received_time[0] 
                    || !status.value.port_link_status()[3] {
                        slv.clock.received_time[i] = 0; 
                    }
                }
            }
        }).collect::<Vec<_>>().join().await;

        // Compute delay between slave (ordered by index)
        self.update_delay().await;

        // Compute offset between localtime and slave reference local time
        self.update_offset(local_time).await;

        // these hardcoded constants are from the ETG.1020 22.2.4
        for slv in self.slaves.iter_mut()  {
            slv.clock.control_loop_params[0] = 0x1000u16;
            slv.clock.control_loop_params[2] = u16::from_le_bytes([0x00u8, 0x0cu8]);
        }

        // Send transmission delay in parallel, offset and isochronous config
        self.slaves.iter().map(|slv| async {
            //Ignore reference and non active slave
            if slv.address >= self.referent() {
                // Send offset and delay
                self.master.fpwr(slv.address, registers::dc::rcv_time_delay, slv.clock.system_delay).await.one();
                self.master.fpwr(slv.address, registers::dc::rcv_time_offset, slv.clock.system_offset).await.one();

                // Enable sync unit
                self.master.fpwr(slv.address, registers::dc::rcv_time_loop_2, slv.clock.control_loop_params[2]).await.one();
                self.master.fpwr(slv.address, registers::dc::rcv_time_loop_0, slv.clock.control_loop_params[0]).await.one();
            }
        }).collect::<Vec<_>>().join().await;

        // Static drif correction
        println!("static drift");
        for _ in 0..self.drift_frm_count {
            let p = self.master.bwr(registers::dc::rcv_system_time, self.global_time());
            self.master.flush();
            p.await;
        }

        println!("initialized");
        self.status = ClockStatus::Initialized;
        Ok(())
    }

    /**
        Start dc synchronisation. Use cycle time as execution period
        
        Once the automatic control is start, time cannot period cannot be changed.
        Start cyclic time correction only with conitnuous drift flag set.
    */
    pub async fn sync(&mut self) -> Result<(), EthercatError> {
        let start_time = Duration::from_millis(1);
        println!("start sync");
        
        if self.status != ClockStatus::Initialized 
            {return Err(EthercatError::Master("Cannot run DC clock synchro until delay wasn't computed"))}

        self.status = ClockStatus::Active;
        self.typ = MasterSyncType::Dc;

        // Start sync - Wait that all slave obtains the frame
        let t = self.global_time() + start_time.as_nanos() as u64;
        self.slaves.iter_mut().map(|slv| async {
            self.master.fpwr(slv.address, registers::isochronous::slave_sync, slv.isochronous.sync).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_start_time, t as u32).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_sync_time_0, slv.isochronous.sync_0_cycle_time).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_sync_time_1, slv.isochronous.sync_1_cycle_time).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_latch_edge_0, slv.isochronous.latch_0_edge).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_latch_edge_1, slv.isochronous.latch_1_edge).await;
        }).collect::<Vec<_>>().join().await;

        thread_priority::set_current_thread_priority(ThreadPriority::Max).unwrap();

        let mut interval = tokio::time::interval(Duration::from_nanos(self.cycle_time));
        let mut watched_slave_idx = 0;

        // Wait the DC synchro start
        // TODO: see if this is necessary
        while self.global_time() < t {}

        println!("sync");
        interval.reset();
        while self.status == ClockStatus::Active {
            // Wait tick
            interval.tick().await;

            // Continuous drift correction and survey
            if self.status == ClockStatus::Active && self.dc_correction_typ == DCCorrectionType::Continuous {
                let dt = self.global_time();
                
                // Send data in the same frame
                let (_, time_diff, _, _) = (
                    self.master.bwr(registers::dc::rcv_system_time, dt),
                    self.master.fprd(self.slaves[watched_slave_idx].address, registers::dc::rcv_time_diff),
                    self.master.brd(registers::dls_user::r3),
                    // send anything just to have a flushing pdu sending
                    self.master.pdu(PduCommand::BRD, SlaveAddress::Broadcast, registers::dls_user::r3.byte as u32, &mut [0u8; 4], true),
                ).join().await;
                
                self.slaves[watched_slave_idx].clock.system_difference = time_diff.one();
                
                self.survey_time_sync();

                watched_slave_idx += 1;
                watched_slave_idx %= self.slaves.len();
            }

            // TODO: Notify main thread that cycle occured ?
            self.master.flush();
        }
        Ok(())
    }
    
    fn survey_time_sync(&mut self) {
        for slv in self.slaves.iter_mut() {
            let desync = i32::from(slv.clock.system_difference);
            print!("{desync} ");
        }
        print!("\n");
    }

    /**
        Send a reset clock to all slave
        
        If status is not in REGISTER, also reset DC+Isochronous clock
    */
    pub async fn reset(&mut self) {
        if self.status == ClockStatus::Active {
            self.status = ClockStatus::Initialized;
        }

        if self.status != ClockStatus::Registering {
            for slv in self.slaves.iter_mut() {
                slv.clock = DistributedClock::default();
                slv.isochronous = Isochronous::default();
            }
        }
        (
            self.master.bwr(registers::dc::clock, DistributedClock::default()),
            self.master.bwr(registers::isochronous::slave_cfg, Isochronous::default()),
        ).join().await;
    }
    
    /// time offset between the master (the computer's system clock, aka local time) and the reference clock
    pub fn offset_master(&self) -> i128   {self.offset}
    /// time offset between the given slave clock and the reference clock
    pub fn offset_slave(&self, slave: u16) -> i128   {
        i32::from_ne_bytes(
            self.slaves.iter()
                .find(|s| s.address == slave).unwrap()
                .clock.system_delay
                .to_ne_bytes()
            ).into()
    }
    /// return the current time on the reference clock
    pub fn reference(&self) -> Instant  {
        Instant::now() + Duration::from_nanos(u64::from(self.slaves[0].clock.system_delay))
    }
    /// return the slave address of the slave used as reference clock. This slave is called referent, or reference slave.
    pub fn referent(&self) -> u16  {self.slaves.first().unwrap().address}
    
    /// Get DC instance status
    pub fn status(&self) -> ClockStatus {self.status}

}
    
// ==============================    Internal tools   ==============================
impl SyncClock {
    /**
        Configure DC for the specific slave. See Isochronous struct for more detail 981 register
        
        - *slave* :  Slave address
        - *config* : Slave configuration struct
    */
    fn configure_slave(&mut self, slave: u16, config: SlaveClockConfigHelper) {
        for slv in self.slaves.iter_mut() {
            if slave != slv.address  {continue}

            slv.clock_type = SlaveSyncType::Free;
            if ! config.sync_0_time.is_none() {
                slv.isochronous.sync.set_generate_sync0(true);
                slv.isochronous.interrupt1.set_interrupt(true);
                slv.isochronous.sync_0_cycle_time = config.sync_0_time.unwrap();
                slv.clock_type = SlaveSyncType::DcSync0;
                slv.isochronous.latch_0_pos_edge_value = u32::from(config.pos_latch_flg.unwrap_or_default());
                slv.isochronous.latch_0_neg_edge_value = u32::from(config.neg_latch_flg.unwrap_or_default());
            }
            if ! config.sync_1_time.is_none() {
                slv.isochronous.sync.set_generate_sync1(true);
                slv.isochronous.interrupt2.set_interrupt(true);
                slv.isochronous.sync_1_cycle_time = config.sync_1_time.unwrap();
                slv.clock_type = SlaveSyncType::DcSync1;
                slv.isochronous.latch_1_pos_edge_value = u32::from(config.pos_latch_flg.unwrap_or_default());
                slv.isochronous.latch_1_neg_edge_value = u32::from(config.neg_latch_flg.unwrap_or_default());
            }
            slv.isochronous.sync.set_enable_cyclic(slv.clock_type >= SlaveSyncType::DcSync0);
        }
    }


    /// Compute for each slave of the current group the system time latency between local clock and reference,
    /// internal prcessing delay and net frame delay transmission.
    /// At the computation purpose,  transmute T and "X" slaves branch to a virtual straight line by trimming
    /// See for much detail
    async fn update_delay(&mut self) {

        // Compute delay between two successive master/slave - begin from the last slave of the loop
        let mut prv_slave: Option<&SlaveInfo> = None;
        for slv in self.slaves.iter_mut().rev() {
            if prv_slave.is_some() && slv.clock.received_time[1] != 0 {
                let previous = prv_slave.unwrap().clock;
                if previous.received_time[3] != 0 {
                    slv.clock.system_delay = (
                        (slv.clock.received_time[1] - slv.clock.received_time[0])
                        - (previous.received_time[3] - previous.received_time[2])
                        - (previous.received_time[2] - previous.received_time[1])
                        ) / 2;
                } else if previous.received_time[2] != 0 {
                    slv.clock.system_delay = (
                        (slv.clock.received_time[1] - slv.clock.received_time[0])
                        - (previous.received_time[2] - previous.received_time[1])
                        ) / 2;
                } else if previous.received_time[1] != 0 {
                    slv.clock.system_delay = 
                        (slv.clock.received_time[1] - slv.clock.received_time[0]) / 2;
                }
            }
            else {
                // No time set -> no dc ?
            }
            prv_slave = Some(slv);
        }

        // Compute transmition latence with the reference + notify slave from delay
        self.delay = self.slaves.first().unwrap().clock.system_delay;
        self.slaves.first_mut().unwrap().clock.system_delay = 0;
        let referent = self.referent();
        for slv in self.slaves.iter_mut() {
            if slv.address != referent && self.delay > slv.clock.system_delay {
                slv.clock.system_delay = self.delay - slv.clock.system_delay;
            }
        }
    }

    /// Apply a correction on DC offset drift<br>
    /// This operation must be done after delay computation
    /// - *local_time*: Local time when BRW 900 was set
    async fn update_offset(&mut self, local_time : u64) {
        //Get and store local ref time
        let ref_time = self.slaves.first().unwrap().clock.local_time;
        self.offset = i128::from(local_time) - i128::from(ref_time) - i128::from(self.delay);

        dbg!(self.offset);
        // Compute offset
        for slv in self.slaves.iter_mut() {
            //Ignore slave before reference
            if slv.clock_type >= SlaveSyncType::DcSync0 {
                slv.clock.system_offset = (i128::from(ref_time) - i128::from(slv.clock.local_time) - i128::from(slv.clock.system_delay)) as u64;
            }
        }
    }


    /// Get a system time in nano seconds
    fn local_time(&self) -> u64 {
        self.start.elapsed().as_nanos().try_into().unwrap()
    }

    // Get DC global time in nano seconds
    fn global_time(&self) -> u64 {
        if let Some(referent) = self.slaves.first() {
            (
            i128::from(self.local_time())
            - self.offset
            - i128::from(referent.clock.system_delay)
            ).try_into().unwrap()
        }
        else { 
            self.local_time()
        }
    }

//     /// Shift time used to synchronoize DC frame send for each slave. Time must be in ns.
//     pub fn set_shift(&mut self, shift: u32) {
//         self.shift = shift;
//     }
}

impl SlaveInfo{
    fn new(address: u16) -> Self {
        Self {
            address,
            clock: DistributedClock::default(),
            isochronous : registers::isochronous::Isochronous::default(),
            clock_type : SlaveSyncType::Free,
        }
    }
}




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
    /// SM2 + Shift Input Latch
    Sm2Shif,
    /// SM3 + Shift Output Latch
    Sm3Shift,
    /// DC - Standard
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
}

#[derive(Debug, PartialEq, Eq, Default, Copy, Clone)]
pub enum ClockStatus {
    #[default]
    Registering,
    Initialized,
    Active
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
    /// Enable positive single edge (P7) - Activate it for SYNC 0 or SYNC 1
    pub pos_latch_flg : Option<bool>,
    /// Enable negative single edge (P7) - Activate it for SYNC 0 or SYNC 1
    pub neg_latch_flg : Option<bool>,
}

impl Default for SlaveClockConfigHelper {
    fn default() -> Self { Self { 
        // sync_0_time = 20000ns [0x1e8480]
        sync_0_time: Some(2000000), 
        sync_1_time: None, 
        cyclic_start_time: None, 
        pos_latch_flg: None, 
        neg_latch_flg: None,
    }}
}

/// Use to implement advanced clock synchronization through syncmanager (mailbox is required)
pub struct SynchronizationSdoHelper {}

impl SynchronizationSdoHelper {

    /// Send data for configurate the specified synchronisation chanel. Send sdo for the register `0x1C32`+ channel`.
    pub async fn send(
        sync_type: SlaveSyncType, 
        cycle_time: Option<u32>, 
        shift_time: Option<u32>, 
        get_cycle_time: Option<(bool,bool)>, 
        slv: &Slave<'_>, 
        channel: u16,
    ) -> Result<(), EthercatError<&'static str>> {
        let status : AlState = slv.state().await;
        if status == AlState::SafeOperational || status == AlState::Operational {

            if channel > 32 { return Err(EthercatError::Protocol("Channel to high")); }

            let mut canoe = slv.coe().await;
            let sdo_sync = sdo::Synchronization {index: 0x1C32 + channel};
            let typ = match sync_type {
                SlaveSyncType::Free => sdo::SyncType::none(),
                SlaveSyncType::Sm2 => sdo::SyncType::synchron(),
                SlaveSyncType::Sm3 => sdo::SyncType::sync_manager(channel as u8),
                SlaveSyncType::DcSync0 => sdo::SyncType::dc_sync0(),
                SlaveSyncType::DcSync1 => sdo::SyncType::dc_sync1(),
                _ => sdo::SyncType::none(),
            };

            // Check capability or return error
            let smf = Self::receive(slv, channel).await.expect("Error channel not available");
            let capability = sdo::SyncSupportedMode::from(smf.supported_sync_type);
            if capability.sm()  && (sync_type == SlaveSyncType::Sm2 && sync_type == SlaveSyncType::Sm3 ) 
                {return Err(EthercatError::Protocol("Synchronization not supported by this slave"))}
            if capability.dc_sync0() && sync_type == SlaveSyncType::DcSync0 
                {return Err(EthercatError::Protocol("Synchronization not supported by this slave"))}
            if capability.dc_sync1() && sync_type == SlaveSyncType::DcSync1 
                {return Err(EthercatError::Protocol("Synchronization not supported by this slave"))}
            if capability.dc_fixed() && sync_type == SlaveSyncType::Subordinate 
                {return Err(EthercatError::Protocol("Synchronization not supported by this slave"))}

            // Write synchronization sdo
            canoe.sdo_write(&sdo_sync.ty(), u2::new(1), typ).await;
            if cycle_time.is_some() {
                canoe.sdo_write(&sdo_sync.period(), u2::new(1), cycle_time.unwrap()).await; 
            }
            if shift_time.is_some() {
                canoe.sdo_write(&sdo_sync.shift(), u2::new(1), shift_time.unwrap()).await; 
            }
            if capability.dc_sync0() || capability.dc_sync1() || capability.dc_fixed() && get_cycle_time.is_some() {
                let mut data = sdo::SyncCycleTimeDsc::default();
                data.set_measure_local_time(get_cycle_time.unwrap().0);
                data.set_reset_event_counter(get_cycle_time.unwrap().1);
                canoe.sdo_write(&sdo_sync.toggle_lt(), u2::new(1), data).await; 
            }
        }
        Ok(())
    }

    /// Receive synchronization sdo for the specific channel. Receive sdo for the register `0x1C32`+ channel`.
    /// Return a None if `channel > 32`
    pub async fn receive(slv: &Slave<'_>, channel: u16) -> Option<SyncMangerFull> {
        if channel > 32   {return None}
        let sdo_id = Sdo::<SyncMangerFull>::complete(0x1C32 + channel);
        Some( slv.coe().await
                .sdo_read(&sdo_id, u2::new(1)).await )
    }
}



