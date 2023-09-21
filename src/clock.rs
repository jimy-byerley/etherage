/*!
    Implementation of clock synchonization between master and slaves.

    Clock synchronization in the ethercat protocol serves two different purposes

    - slave task synchronization

        The slaves whose clocks are synchronized will be able to run their realtime tasks with the same time reference (like applying new commands at the same time, or using the same durations), and at the same rate as the master sends order.

    - timestamps synchronization

        The slaves whose clocks are synchronized will progress at the same rate (slight differences can be found due to synchronization jitter, but it will remain small), so data retreived from slaves will have been measured at the same time and to retreive one only timestamp per frame will be sufficient for these slaves.

    All this is described in ETG.1000.4 + ETG.1000.6 and ETG.1020

    ## synchronization modes

    There is 3 modes of slave task synchronization:

    - **free run**
        slaves tasks are not synchronized to ethercat. This mode is when no clock is initialized
    - **SM-synchronous**
        slaves tasks are triggered by an ethercat sending
    - **DC-synchronous**
        slaves tasks are triggered by their clock synchronized with other slaves and the master. This mode is implemented in [SyncClock]
        according to ETG.1020, this mode is required only for the application that require high precision (<ms) in operation.
        
        Multiple mode of DC synchronous for DC unit in slave are available. The default one used only the sync_0 impulse to trigger based time. (see [this](/etherage/schemes/synchronization-DC-submodes.svg) schematoic to get more information)


    ![synchronization modes](/etherage/schemes/synchronization-modes.svg)

    Depending on the synchronization mode, you can expect different execution behavior on your slaves, whose importance higher with the number of slaves. The following chronogram shows typical scheduling of task executions.

    ![synchronization of slaves](/etherage/schemes/synchronization-slaves.svg)

    ## roles and responsibilities in the ethercat network

    Since the master can be connected to the ethercat segment using less reliable hardware, its clock cannot be used to synchronize slaves. Instead, the first slave supporting DC (distributed clock) is used as reference clock (the first slave is the called *referent*).
    the reference clock time is used to monitor the jitter between master and referent, and the jitter between all slaves.

    In case of hotplug, or any change in the transmission delays in the segment, the clock must be reinitialized.
*/

use crate::{
    registers::{self, AlState},
    sdo::{self, SyncMangerFull},
    rawmaster::{RawMaster, PduCommand, SlaveAddress},
    EthercatError, Slave, Sdo,
    };
use std::{
    collections::HashMap,
    time::{Instant, Duration},
    sync::Arc,
    };
use core::sync::atomic::{
    AtomicBool, AtomicU8,
    Ordering::*,
    };

use bilge::prelude::*;
use futures_concurrency::future::Join;
use thread_priority::*;



/// Default frame count used to set static drift synchronization, recommented by ETG.1020
const STATIC_FRAME_COUNT: usize = 15000;
/// Default time used to trigger system time synchronization (ns)
const CONTINOUS_TIMELAPS: u64 = 2000000;
/// Value required to enable the DC clock - see ETG.1020 - 22.2.4
const DC_CONTROL_LOOP_0_RESET: u16 = 0x1000u16;
const DC_CONTROL_LOOP_2_STARTUP: u16 = u16::from_le_bytes([0x04u8, 0x0Cu8]);
const DC_CONTROL_LOOP_2_ADJUST:  u16 = u16::from_le_bytes([0x00u8, 0x0cu8]);


/**
    implementation of the Distributed Clock (DC) at the master level.

    Other kinds of clock do not concern this struct (at least for now).

    The time offsets and delays measured by this clock synchronization mode are shows in the following chronogram for one packet sending.

    ![clock offsets references](/etherage/schemes/clock-references.svg)
*/
pub struct SyncClock {
    // Raw master reference
    master: Arc<RawMaster>,
    /// start instant used as master's clock
    start: Instant,
    /// flag stopping synchronization execution
    stopped: AtomicBool,

    /// Time shift between master local time and reference local time (this is the difference between the clocks for the same instant, without transmission delay)
    offset: i128,
    /// Delay transmision between master and reference
    delay: u32,

    /// Type of DC used
    dc_correction_typ: DCCorrectionType,
    /// Array of registerd slave
    slaves: Vec<SlaveInfo>,
    /// index into [Self::slaves], allowing to retreive slave index by address
    index: HashMap<u16, usize>,
    /// DC control cycle time
    cycle_time: u64,
    /// Number of frame used to prevent static drift
    drift_frm_count: usize,
    /// Current status of the SyncClock structure
    status: ClockStatus,
    /// Current master synchronization type
    typ: MasterSyncType,
    /// flag declaring that variables are updated by the synchronization task
    updating: AtomicU8,
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
            stopped: AtomicBool::new(false),
            offset: 0,
            delay: 0,
            dc_correction_typ: DCCorrectionType::Continuous,
            slaves: Vec::new(),
            index: slaves.iter().cloned().enumerate()
                    .map(|(i,s)|  (s,i)).collect(),
            cycle_time: period
                            .map(|t| t.as_nanos() as u64)
                            .unwrap_or(CONTINOUS_TIMELAPS),
            drift_frm_count: static_frames.unwrap_or(STATIC_FRAME_COUNT),
            status: ClockStatus::Registering,
            typ: MasterSyncType::Cyclic,
            updating: Default::default(),
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
            .inspect(|(slave, _)|  assert_ne!(*slave, 0, "clock synchronization without fixed addresses is unsafe"))
            .map(|(slave, _)| slave.clone())
            .collect::<Vec<u16>>();

        Self::new(master, slaves, period, static_frames).await
    }

    /// return the ethercat master this clock is working with
    pub unsafe fn raw_master(&self) -> &Arc<RawMaster>  {&self.master}

    /**
        configure slaves, compute delays and static drifts

        *index*: Index of the slave used as reference (must be the first slave of the physical loop)

        Warning: This method uses dynamic allocation don't use it in realtime operations

        Return an error:
            - In case of slave reference doesn't register in this DC instance
            - In case of command execution fail
    */
    async fn init(&mut self) -> Result<(), EthercatError> {
        // Check number of slave declared
        let local_time = self.local_time();
        if self.master.brw(registers::dc::rcv_time_brw, 0).await.answers == 0
            {return Err(EthercatError::Master("DC initialisation - No slave found"))}

        // no reset is done since it is the responsibility of the caller
        // for a protocol-safe initialization with guaranteed reset, use [Master::init_clock] that will perform the reset

        // ETG.1020 - 22.2.4 Master Control loop - Initialize loop parameter for all DC node
        // these hardcoded constants are from the specs
        self.master.bwr(registers::dc::rcv_time_loop_2, DC_CONTROL_LOOP_2_STARTUP).await;
        self.master.bwr(registers::dc::rcv_time_loop_0, DC_CONTROL_LOOP_0_RESET).await;
        for slv in self.slaves.iter_mut() {
            slv.clock.control_loop_params[0] = DC_CONTROL_LOOP_0_RESET;
            slv.clock.control_loop_params[2] = DC_CONTROL_LOOP_2_STARTUP;
        }

        // For each engine read rcv local time, port rcv time and status
        // Collect many values of DC
        const ARRAY_MAX_SIZE: usize = 8;
        let mut dc_vec: [Vec<registers::dc::DistributedClock>; ARRAY_MAX_SIZE] = Default::default();
        for i in 0 .. dc_vec.len() {
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
            for i in 0 .. dc_vec.len() {
                for k in 0 .. rcvt.len() { rcvt[k] += u128::from(dc_vec[i][j].received_time[k]); }
                st += u128::from(dc_vec[i][j].system_time);
                lt += u128::from(dc_vec[i][j].local_time);
            }
            for k in 0 .. rcvt.len() {
                self.slaves[j].clock.received_time[k] = (rcvt[k] / dc_vec.len() as u128) as u32;
            }
            self.slaves[j].clock.system_time = (st / dc_vec.len() as u128) as u64;
            self.slaves[j].clock.local_time = (lt / dc_vec.len() as u128) as u64;
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
            slv.clock.control_loop_params[0] = DC_CONTROL_LOOP_0_RESET;
            slv.clock.control_loop_params[2] = DC_CONTROL_LOOP_2_ADJUST;
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
        Ok(())
    }

    /**
        distributed clock synchronisation task. Using cycle time as execution period

        Once the automatic control is start, time cannot period cannot be changed.
        Start cyclic time correction only with conitnuous drift flag set.

        One task only should run this function at the same time
    */
    pub async fn sync(&self) -> Result<(), EthercatError> {
        let start_time = Duration::from_millis(1);

        // Start sync - Wait that all slave obtains the frame
        let t = self.global_time() + start_time.as_nanos() as u64;
        self.slaves.iter().map(|slv| async {
            self.master.fpwr(slv.address, registers::isochronous::slave_sync, slv.isochronous.sync).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_start_time, t as u32).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_sync_time_0, slv.isochronous.sync_0_cycle_time).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_sync_time_1, slv.isochronous.sync_1_cycle_time).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_latch_edge_0, slv.isochronous.latch_0_edge).await;
            self.master.fpwr(slv.address, registers::isochronous::slave_latch_edge_1, slv.isochronous.latch_1_edge).await;
        }).collect::<Vec<_>>().join().await;

        thread_priority::set_current_thread_priority(ThreadPriority::Max).unwrap();

        let mut interval = tokio::time::interval(Duration::from_nanos(self.cycle_time));
        let mut watched = 0;

        // Wait the DC synchro start
        // TODO: see if this is necessary
        while self.global_time() < t {std::thread::yield_now()}

        interval.reset();
        while ! self.stopped.load(Relaxed) {
            // Wait tick
            interval.tick().await;

            // Continuous drift correction and survey
            if self.dc_correction_typ == DCCorrectionType::Continuous {
                let dt = self.global_time();

                // Send data in the same frame
                // survey one slave each iteration
                // TODO: offer more survey options
                let (_, time_diff, _, _) = (
                    self.master.bwr(registers::dc::rcv_system_time, dt),
                    self.master.fprd(self.slaves[watched].address, registers::dc::rcv_time_diff),
                    self.master.brd(registers::dls_user::r3),
                    // send something just to have a flushing pdu sending
                    self.master.pdu(PduCommand::BRD, SlaveAddress::Broadcast, registers::dl::status.byte as u32, &mut [0u8; 2], true),
                ).join().await;

                let time_diff = time_diff.one();
                self.store_updating(|| unsafe {
                    (&mut *(&self.slaves[watched] as *const _ as *mut SlaveInfo)).clock.system_difference = time_diff;
                });

                // only here for debug
//                 self.survey_time_sync();

                watched += 1;
                watched %= self.slaves.len();
            }

            // TODO: Notify main thread that cycle occured ?
            self.master.flush();
        }
        Ok(())
    }

    /// stop execution of [Self::sync]
    pub fn stop(&self) {
        self.stopped.store(true, Relaxed);
    }

    /// Get DC instance status
    pub fn status(&self) -> ClockStatus {self.status}

    /// return the slave address of the slave used as reference clock. This slave is called referent, or reference slave.
    pub fn referent(&self) -> u16  {self.slaves.first().unwrap().address}
    /// return the current time on the reference clock
    pub fn reference(&self) -> Instant  {
        Instant::now() + Duration::from_nanos(u64::from(self.slaves.first().unwrap().clock.system_delay))
    }

    /// time offset between the master (the computer's system clock, aka local time) and the reference clock
    pub fn offset_master(&self) -> i128   {self.offset}
    /// time offset between the given slave clock and the reference clock
    pub fn offset_slave(&self, slave: u16) -> i128   {
        i32::from_ne_bytes(
            self.slaves[self.index[&slave]]
                .clock.system_delay
                .to_ne_bytes()
            ).into()
    }
    /// return the transmission delay from the master to the given slave
    pub fn delay(&self, slave: u16) -> i128  {
        self.slaves[self.index[&slave]]
            .clock.system_delay
            .into()
    }
    /**
        return the current synchronization error (time gap) between the reference clock and the given slave clock.
        If perfectly synchronized, this value should be `0` for all slaves even with a transmission delay between slaves
    */
    pub fn divergence(&self, slave: u16) -> i128  {
        let clock = self.slaves[self.index[&slave]].clock;
        i32::from(self.load_updating(|| clock.system_difference)).into()
    }
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

    // synchronization mechanism for read/writting internal data

    /// wrap a blocking reading of internal data, no blocking is done if nothing is being written
    fn load_updating<T, F>(&self, mut task: F) -> T
    where F: FnMut() -> T
    {
        loop {
            let mut update;
            loop {
                update = self.updating.load(Acquire);
                if update & WRITING == 0 {break}
                std::thread::yield_now();
            }
            let result = task();
            if self.updating.load(Acquire) == update {break result}
        }
    }

    /// wrap a wait-free writting of internal data, [Self::load_updating] will be blocked during this call
    fn store_updating<F>(&self, mut task: F)
    where F: FnMut()
    {
        let update = self.updating.load(Acquire) + 1;
        self.updating.store(update | WRITING, Release);
        task();
        self.updating.store(update & !WRITING, Release);
    }
}

const WRITING: u8 = 0b1<<7;

impl SlaveInfo {
    fn new(address: u16) -> Self {
        Self {
            address,
            clock: Default::default(),
            isochronous: Default::default(),
            clock_type: SlaveSyncType::Free,
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
        sync_0_time: Some(CONTINOUS_TIMELAPS as u32),
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
