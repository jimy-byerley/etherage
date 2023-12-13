/*!
    Implementation of clock synchonization between master and slaves.

    Clock synchronization in the ethercat protocol serves two different purposes

    - slave task synchronization

        The slaves whose clocks are synchronized will be able to run their realtime tasks with the same time reference (like applying new commands at the same time, or using the same durations), and at the same rate as the master sends order.

    - timestamps synchronization

        The slaves whose clocks are synchronized will progress at the same rate (slight differences can be found due to synchronization jitter, but it will remain small), so data retreived from slaves will have been measured at the same time and to retreive one only timestamp per frame will be sufficient for these slaves.

    All this is described in ETG.1000.4 + ETG.1000.6 and ETG.1020.21

    ## synchronization modes

    There is 3 modes of slave task synchronization (ETG.1020.21.1.1):

    - **free run**
        slaves tasks are not synchronized to ethercat. This mode is when no clock is initialized
    - **SM-synchronous**
        slaves tasks are triggered by an ethercat sending
    - **DC-synchronous**
        slaves tasks are triggered by their clock synchronized with other slaves and the master. This mode is implemented in [DistributedClock]
        according to ETG.1020, this mode is required only for the application that require high precision (<ms) in operation.
        
        Multiple mode of DC synchronous for DC unit in slave are available. The default one used only the sync_0 impulse to trigger based time. (see [this](/etherage/schemes/synchronization-DC-submodes.svg) schematic to get more information)


    ![synchronization modes](/etherage/schemes/synchronization-modes.svg)

    Depending on the synchronization mode, you can expect different execution behavior on your slaves, whose importance higher with the number of slaves. The following chronogram shows typical scheduling of task executions.

    ![synchronization of slaves](/etherage/schemes/synchronization-slaves.svg)

    ## roles and responsibilities in the ethercat network

    Since the master can be connected to the ethercat segment using less reliable hardware, its clock cannot be used to synchronize slaves. Instead, the first slave supporting DC (distributed clock) is used as reference clock (the first slave is the called *referent*).
    the reference clock time is used to monitor the jitter between master and referent, and the jitter between all slaves.

    In case of hotplug, or any change in the transmission delays in the segment, the clock must be reinitialized.
*/

use crate::{
    data::PduData,
    registers::{self, AlState},
//     sdo::{self, Sdo},
    rawmaster::{RawMaster, PduCommand, SlaveAddress},
    error::{EthercatError, EthercatResult}, 
    can::CanError,
    Slave,
    };
use std::{
    collections::HashMap,
    time::{SystemTime, Instant, Duration},
    sync::Arc,
    };
// use core::sync::atomic::{
//     AtomicBool, AtomicI64,
//     Ordering::*,
//     };

use futures_concurrency::future::Join;





/**
    implementation of the Distributed Clock (DC) at the master level.

    The time offsets and delays measured by this clock synchronization mode are shows in the following chronogram for one packet sending.

    ![clock offsets references](/etherage/schemes/clock-references.svg)
*/
pub struct DistributedClock {
    // Raw master reference
    master: Arc<RawMaster>,
    
    /// start instant used as master's clock, it serves as monotonic clock for all offset computations to guarantee the synchronization success
    start: Instant,
    /// system clock when the clock has been initialized, it serves as reference to return an offset between slaves clock and system clock. If the system clock has not been monotonic all the time since the initialization of this instance, any offset from slave to master will not mean anything to the user
    epoch: SystemTime,
    
    /// topological index of clock reference slave
    referent: usize,
    /// per-slave variables, slaves are indexed by topological order
    slaves: Vec<ClockSlave>,
    /// topological position of slaves indexed by fixed address (for user needs)
    index: HashMap<SlaveAddress, usize>,
}
#[derive(Debug)]
struct ClockSlave {
	/// fixed address of slave
	address: SlaveAddress,
	/// whether the slave supports DC
	enabled: bool,
	/// topological index of slaves connected to each port
	topology: [Option<usize>; 4],
	/// offset from local time to system clock
	offset: i64,
	/// transmission delay from clock reference slave to present slave
	delay: u32,
// 	/// last measured difference between local estimated system time and received system time
// 	divergence: AtomicI64,
}

	
type DLSlave = Vec<(u16, registers::DLInformation, registers::DLStatus)>;


impl DistributedClock {
	/**
		initialize the distributed clock on the ethercat segment
		
		Since the slaves responsibilities in the distributed clock only depend from the topology of the network, this initializer will automatically detect the topology and act accordingly
		
		## parameters
		
		- `delays_samples`: number of clock sampling used in estimating the propagation delays between slaves (defaults to `8`)
		- `offsets_samples`: number of clock sampling used in estimating the offsets between slaves clocks (defaults to `15_000`)
	*/
	pub async fn new(
			master: Arc<RawMaster>,
			delays_samples: Option<usize>,
			offsets_samples: Option<usize>,
			) -> EthercatResult<Self> {
		// create struct
		let mut clock = Self {
			master,
            
            start: Instant::now(),
            epoch: SystemTime::now(),
            
            referent: 0,
            slaves: Vec::new(),
            index: HashMap::new(),
			};
		let infos = clock.init_slaves().await?;
		clock.init_topology(&infos).await?;
		clock.init_delays(&infos, delays_samples.unwrap_or(8)).await?;
		clock.init_offsets(offsets_samples.unwrap_or(15_000)).await?;
		
		dbg!(&clock.slaves);
		Ok(clock)
	}
	
	async fn init_slaves(&mut self) -> EthercatResult<DLSlave> {
		// check number of slaves
        let support = self.master.brd(registers::dl::information).await;
        if support.answers == 0 || ! support.value()?.dc_supported()
            {return Err(EthercatError::Master("no slave supporting clock"))}
		
		// retreive informations about all slaves in the network
		let master = self.master.as_ref();
		let infos = (0 .. support.answers).map(|slave| async move {
				let (address, support, status) = (
					master.aprd(slave, registers::address::fixed),
					master.aprd(slave, registers::dl::information),
					master.aprd(slave, registers::dl::status),
					).join().await;
				Ok((address.one()?, support.one()?, status.one()?))
			})
			.collect::<Vec<_>>()
			.join().await
			.drain(..).collect::<EthercatResult<Vec<_>>>()?;
		
		// check addresses and dc-enabled slaves
		self.slaves = infos.iter().enumerate()
			.map(|(index, (fixed, information, _))| ClockSlave {
				address: 
					if *fixed == 0  {SlaveAddress::AutoIncremented(index as _)}
					else           {SlaveAddress::Fixed(*fixed)},
				enabled: 
					information.dc_supported(),
				topology: [None; 4],
				offset: 0,
				delay: 0,
// 				divergence: AtomicI64::new(0),
			})
			.collect::<Vec<_>>();
		
		// the reference slave must be the first supporting clock in the network
		self.referent = (0 .. self.slaves.len())
			.find(|index|  self.slaves[*index].enabled)
			.ok_or(EthercatError::Protocol("cannot find first slave supporting clock"))?;
			
		Ok(infos)
	}
	
	/// build topology
	async fn init_topology(&mut self, infos: &DLSlave) -> EthercatResult {
		let mut stack = Vec::<usize>::new();
		for index in 0 .. infos.len() {
			if index == 0 {
				self.slaves[index].topology[0] = Some(0);
			}
			else {
				let (parent, port) = loop {
					let Some(&parent) = stack.last()
						else {return Err(EthercatError::Protocol("topology identification failed due to wrong slave port activation"))};
					if let Some(port) = (0 .. self.slaves[parent].topology.len())
							.find(|&port|  infos[parent].2.port_link_status_at(port) && self.slaves[parent].topology[port].is_none()) 
						{break (parent, port)}
					stack.pop();
				};
				self.slaves[parent].topology[port] = Some(index);
				self.slaves[index].topology[0] = Some(parent);
			}
			stack.push(index);
		}
		Ok(())
	}
	/// compute delays
	async fn init_delays(&mut self, infos: &DLSlave, samples: usize) -> EthercatResult {
		// get samples
		let mut stamps = vec![[0; 4]; infos.len()*samples];
		for i in 0 .. samples {
			self.master.bwr(registers::dc::measure_time, 0).await;
			let master = self.master.as_ref();
			for (index, times) in self.slaves.iter()
				.enumerate()
				.filter(|(_, slave)|  slave.enabled)
				.map(|(index, slave)| async move {
					(index, master.read(slave.address, registers::dc::received_time).await)
					})
				.collect::<Vec<_>>()
				.join().await
			{
				stamps[i*samples + index] = times.one()?;
			}
		}
		dbg!(&stamps);
		dbg!(&infos);
		
		// mean samples
		for index in 1 .. self.slaves.len() {
			let parent = self.slaves[index].topology[0].unwrap();
			
			// find enclosing timestamps (activated ports) in parent and child
			let parent_after = self.slaves[parent].topology.iter().enumerate()
				.find(|(_, &next)|  next == Some(index)).unwrap().0;
			let parent_before = self.slaves[parent].topology[0 .. parent_after].iter().enumerate().rev()
				.find(|(_, &next)|  next.is_some()).unwrap().0;
				
			let child_before = 0;
			let child_after = self.slaves[index].topology.iter().enumerate().rev()
				.find(|(_, &next)|  next.is_some()).unwrap().0;
			
			// sum of transition delays times from parent to child
			let mut transitions: u64 = 0;
			// sum of branchs delays from parent port 0 to parent slave port
			let mut ports: u64 = 0;
			
			for i in 0 .. samples {
				let child = &stamps[i*samples + index];
				let parent = &stamps[i*samples + parent];
				let transition = parent[parent_after].wrapping_sub(parent[parent_before])
								 - child[child_after].wrapping_sub(child[child_before]);
				let port = parent[parent_before].wrapping_sub(parent[0]);
				// TODO: use intermediate sums for increase the tolerated delay from 4s in total to 4s per branch
				// TODO: take into account that the slaves clocks might be 32bits using [DLInformaton::dc_range]
				// summation is exact since we are using integers
				transitions += u64::from(transition);
				ports += u64::from(port)
			}
			
			self.slaves[index].delay = self.slaves[parent].delay + u32::try_from(
											transitions / (2*(samples as u64)) + ports / (samples as u64)
											).unwrap();
		}
		// send delays
		self.slaves.iter().map(|slave| async {
				self.master.write(slave.address, registers::dc::system_delay, slave.delay).await.one()
			})
			.collect::<Vec<_>>()
			.join().await
			.drain(..).collect::<EthercatResult>()?;
			
		Ok(())
	}
	
	/// compute offsets (static drift compensation)
	async fn init_offsets(&mut self, samples: usize) -> EthercatResult {
		// we will need an immutable reference to self while modifying the indivudual slave structs. This is safe because we will not access these structs concurrently and will not use methods of self that need them
		let clock = self as *mut Self;
		
		// approximate offset first to get the most significant digits because divergence measurement is only 32 bits
		self.slaves.iter_mut()
			.filter(|slave|  slave.enabled)
			.map(|slave| async move {
				let clock = unsafe {&*clock};
				let remote = clock.master.read(slave.address, registers::dc::local_time).await.one()?;
				let local = clock.reduced();
				let offset = local.wrapping_sub(remote);
				clock.master.write(
					slave.address, 
					registers::dc::system_offset, 
					offset,
					).await.one()?;
				slave.offset = i64::from_ne_bytes(offset.to_ne_bytes());
				Ok(())
			})
			.collect::<Vec<_>>()
			.join().await
			.drain(..).collect::<EthercatResult>()?;
		
		// send many samples of system time (master time), the slave will mean it
        for _ in 0 .. samples {
            self.sync().await;
        }
        // retreive divergence and correct offsets
        self.slaves.iter_mut()
			.filter(|slave|  slave.enabled)
			.map(|slave| async move {
				let clock = unsafe {&*clock};
				slave.offset += i64::from(i32::from(clock.master.read(slave.address, registers::dc::system_difference).await.one()?));
				clock.master.write(
					slave.address, 
					registers::dc::system_offset, 
					u64::from_ne_bytes(slave.offset.to_ne_bytes()),
					).await.one()?;
				EthercatResult::<(), ()>::Ok(())
			})
			.collect::<Vec<_>>()
			.join().await 
			.drain(..).collect::<EthercatResult>()?;
		Ok(())
	}
	
	/// getters
	
    /// return the slave address of the slave used as reference clock. This slave is called referent, or reference slave.
    pub fn referent(&self) -> SlaveAddress  {
        self.slaves[self.referent].address
    }
    /// return the current time on the reference clock
    pub fn system(&self) -> i128  {
        self.start.elapsed().as_nanos().try_into().unwrap()
        // TODO: this clock is 64bits on the slaves, so should the master clock be. The epoch shall be changed when the clock overflows
    }
	
	/// like [Self::system] but wrapped to 64 bit according to ETG
	fn reduced(&self) -> u64 {
        u64::try_from( self.start.elapsed().as_nanos() % u128::from(u64::MAX) ).unwrap()
	}
	
    
    /** 
		offset between the ethercat system clock (arbitrarily zeroed) and unix epoch.
		In this implementation, the ethercat system clock zero is when this struct is initialized
	*/
    pub fn epoch(&self) -> i128 {
        self.epoch.duration_since(SystemTime::UNIX_EPOCH).unwrap()
			.as_nanos()
			.try_into().unwrap()
    }

    /// time offset between the given slave clock and the reference clock
    pub fn offset(&self, slave: SlaveAddress) -> i128   {
        self.slaves[self.index[&slave]].offset.into()
    }
    /// return the transmission delay from the master to the given slave
    pub fn delay(&self, slave: SlaveAddress) -> i128  {
        self.slaves[self.index[&slave]].delay.into()
    }
//     /**
//         return the current synchronization error (time gap) between the reference clock and the given slave clock.
//         If perfectly synchronized, this value should be `0` for all slaves even with a transmission delay between slaves
//     */
//     pub fn divergence(&self, slave: SlaveAddress) -> i128  {
// 		self.slaves[self.index[&slave]].divergence.load(SeqCst).into()
//     }
    

    /**
        distributed clock synchronisation step. It must be called periodically to save the distributed clock from divergence
    */
    pub async fn sync(&self) {
		let referent = self.referent();
		let command = match referent {
			SlaveAddress::AutoIncremented(_) => PduCommand::ARMW,
			SlaveAddress::Fixed(_) => PduCommand::FRMW,
			_ => unreachable!(),
		};
		self.master.pdu(
			command, 
			referent, 
			registers::dc::system_time.byte as u32, 
			&mut (0u64).packed().unwrap(), 
// 			&mut self.reduced().packed().unwrap(), 
			true,
			).await; 
		// we don't care if packet is lost, so no error checking here, it will not bother slaves
	}
	
    /**
        distributed clock synchronisation task. Using cycle time as execution period

        Once the automatic control is start, time cannot period cannot be changed.
        Start cyclic time correction only with conitnuous drift flag set.

        One task only should run this function at the same time. It will be calling [Self::sync]
    */
	pub async fn sync_loop(&self, period: Duration) {
        use futures::stream::StreamExt;
        let mut interval = tokio_timerfd::Interval::new_interval(period).unwrap();
        
        loop {
			interval.next().await.unwrap().unwrap();
			self.sync().await;
		}
	}
}

