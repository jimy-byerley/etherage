use std::sync::Arc;
use core::time::Duration;
use etherage::{
    EthernetSocket, RawMaster, 
    Slave, SlaveAddress, CommunicationState,
    data::Field,
    sdo::{self, Sdo, SyncDirection},
    mapping::{self, Mapping, Group},
    registers,
    };

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let master = Arc::new(RawMaster::new(EthernetSocket::new("eno1")?));
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.receive();
    })};
    {
        let master = master.clone();
        std::thread::spawn(move || loop {
            master.send();
    })};
    std::thread::sleep(Duration::from_millis(500));
    
    println!("create mapping");
    let config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slave = mapping.slave(1);
        let statuscom = slave.register(SyncDirection::Read, registers::al::status);
        let mut channel = slave.channel(sdo::SyncChannel{ index: 0x1c12, direction: SyncDirection::Write, num: 10 });
            let mut pdo = channel.push(sdo::Pdo{ index: 0x1600, num: 10 });
                let controlword = pdo.push(Sdo::<ControlWord>::complete(0x6040));
                let target_position = pdo.push(Sdo::<i32>::complete(0x607a));
                let target_mode = pdo.push(Sdo::<OperationMode>::complete(0x6060));
//                 let target_mode = Field::simple(0);
//                 pdo.push(Sdo::<i32>::complete(0x60ff));
                let target_velocity = pdo.push(Sdo::<i32>::complete(0x60ff));
        let mut channel = slave.channel(sdo::SyncChannel{ index: 0x1c13, direction: SyncDirection::Read, num: 10 });
            let mut pdo = channel.push(sdo::Pdo{ index: 0x1a00, num: 10 });
                let statusword = pdo.push(Sdo::<StatusWord>::complete(0x6041));
                let error = pdo.push(Sdo::<u16>::complete(0x603f));
                let current_position = pdo.push(Sdo::<i32>::complete(0x6064));
                let current_torque = pdo.push(Sdo::<i16>::complete(0x6077));
//                 let target_mode = pdo.push(Sdo::<OperationMode>::complete(0x6060));
    println!("done {:#?}", config);
    
    let mut allocator = mapping::Allocator::new(&master);
    let mut group = tokio::sync::Mutex::new(allocator.group(&mapping));
    
    println!("group {:#?}", group);
    println!("fields {:#?}", (statusword, controlword));
    
    let mut slave = Slave::new(&master, SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await;
    slave.set_address(1).await;
    slave.init_mailbox().await;
    slave.init_coe().await;
    slave.switch(CommunicationState::PreOperational).await;
    group.get_mut().configure(&slave).await;
    slave.switch(CommunicationState::SafeOperational).await;
    slave.switch(CommunicationState::Operational).await;
    
    
    let cycle = tokio::sync::Notify::new();
    
    futures::join!(
        async { 
            let mut period = tokio::time::interval(Duration::from_millis(1));
            loop {
                let mut group = group.lock().await;
                period.tick().await;
                group.exchange().await;
                cycle.notify_waiters();
            }
        },
        async {
            let initial = {
                let mut group = group.lock().await;
                
                let initial = group.get(current_position);
                group.set(target_mode, OperationMode::SynchronousPosition);
                group.set(target_position, initial);
                
                println!("error {:x}", group.get(error));
                println!("position  {}", initial);
                initial
            };
        
            println!("switch on");
            switch_on(statusword, controlword, &group, &cycle).await;
            
			let velocity = 3_000_000;
			
			println!("move forward");
			loop {
                cycle.notified().await;
                let mut group = group.lock().await;
                
                let position = group.get(current_position);
                if position >= initial + 100_000_000 {break}
                group.set(target_position, position + velocity);
                println!("    {}", position);
            }
            
			println!("move backward");
            loop {
                cycle.notified().await;
                let mut group = group.lock().await;
                
                let position = group.get(current_position);
                if position <= initial {break}
                group.set(target_position, position - velocity);
                println!("    {}", position);
            }
            
            println!("done");
        },
    );
    
    Ok(())
}



use bilge::prelude::*;
use core::fmt;

/**
bit structure of a status word

| Bit |  Meaning | Presence |
|-----|----------|----------|
| 0	| Ready to switch on	| M
| 1	| Switched on	| M
| 2	| Operation enabled	| M
| 3	| Fault	| M
| 4	| Voltage enabled	| O
| 5	| Quick stop	| O
| 6	| Switch on disabled	| M
| 7	| Warning	| O
| 8	| Manufacturer specific	| O
| 9	| Remote	| O
| 10	| Operation mode specific	| O
| 11	| Internal limit active	| C
| 12	| Operation mode specific (Mandatory for csp, csv, cst mode)	| O
| 13	| Operation mode specific	| O
| 14-15	| Manufacturer specific	| O
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct StatusWord {
    ready_switch_on: bool,
    switched_on: bool,
    operation_enabled: bool,
    fault: bool,
    voltage_enabled: bool,
    quick_stop: bool,
    switch_on_disabled: bool,
    warning: bool,
    reserved: u1,
    remote: bool,
    reserved: u1,
    limit_active: bool,
    reserved: u1,
    reserved: u3,
}
etherage::data::bilge_pdudata!(StatusWord, u16);

impl fmt::Display for StatusWord {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "StatusWord{{")?;
		for (active, mark) in [ (self.ready_switch_on(), "rtso"),
								(self.switched_on(), "so"),
								(self.operation_enabled(), "oe"),
								(self.fault(), "f"),
								(self.voltage_enabled(), "ve"),
								(self.quick_stop(), "qs"),
								(self.switch_on_disabled(), "sod"),
								(self.warning(), "w"),
								(self.remote(), "r"),
								(self.limit_active(), "la"),
								] {
			write!(f, " ")?;
			if active {
				write!(f, "{}", mark)?;
			} else {
				for _ in 0 .. mark.len() {write!(f, " ")?;}
			}
		}
		write!(f, "}}")?;
		Ok(())
	}
}

/**
Control word of a servo drive

| Bit	|	Category	|   Meaning	|
|-------|---------------|-----------|
| 0	|	M	|	Switch on |
| 1	|	M	|	Enable voltage |
| 2	|	O	|	Quick stop |
| 3	|	M	|	Enable operation |
| 4 – 6	|	O	|	Operation mode specific |
| 7	|	M	|	Fault reset |
| 8	|	O	|	Halt |
| 9	|	O	|	Operation mode specific |
| 10	|	O	|	reserved |
| 11 – 15	|	O	|	Manufacturer specific |
*/
#[bitsize(16)]
#[derive(FromBits, DebugBits, Copy, Clone, Eq, PartialEq, Default)]
pub struct ControlWord {
    switch_on: bool,
    enable_voltage: bool,
    quick_stop: bool,
    enable_operation: bool,
    reserved: u3,
    reset_fault: bool,
    halt: bool,
    specific: bool,
    reserved: u1,
    reserved: u5,
}
etherage::data::bilge_pdudata!(ControlWord, u16);

impl fmt::Display for ControlWord {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ControlWord{{") ?;
		for (active, mark) in [ (self.switch_on(), "so"),
								(self.enable_voltage(), "ev"),
								(self.quick_stop(), "qs"),
								(self.enable_operation(), "eo"),
								(self.reset_fault(), "rf"),
								(self.halt(), "h"),
								] {
			write!(f, " ")?;
			if active {
				write!(f, "{}", mark)?;
			} else {
				for _ in 0 .. mark.len() {write!(f, " ")?;}
			}
		}
		write!(f, "}}")?;
		Ok(())
	}
}


/// servodrive control-loop type
#[bitsize(8)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum OperationMode {
    #[default]
	Off = 0,
	ProfilePosition = 1,
	Velocity = 2,
	ProfileVelocity = 3,
	TorqueProfile = 4,
	Homing = 6,
	InterpolatedPosition = 7,
	
	/// CSP
	SynchronousPosition = 8,
	/// CSV
	SynchronousVelocity = 9,
	/// CST
	SynchronousTorque = 10,
	SynchronousTorqueCommutation = 11,
}
etherage::data::bilge_pdudata!(OperationMode, u8);

pub async fn switch_on(
		statusword: Field<StatusWord>, 
		controlword: Field<ControlWord>, 
		group: &tokio::sync::Mutex<Group<'_>>,
		cycle: &tokio::sync::Notify,
		) {
	loop {
		cycle.notified().await;
		let mut group = group.lock().await;
		
		let status = group.get(statusword);
		let mut control = ControlWord::default();
		
		// state "not ready to switch on"
		if ! status.quick_stop() {
			// move to "switch on disabled"
			control.set_quick_stop(true);
			control.set_enable_voltage(true);
		}
		// state "switch on disabled"
		else if status.switch_on_disabled() {
			control.set_quick_stop(true);
			control.set_enable_voltage(true);
			control.set_switch_on(false);
		}
		// state "ready to switch on"
		else if ! status.switched_on() && status.ready_switch_on() {
			// move to "switched on"
			control.set_quick_stop(true);
			control.set_enable_voltage(true);
			control.set_switch_on(true);
			control.set_enable_operation(false);
		}
		// state "ready to switch on" or "switched on"
		else if ! status.operation_enabled() && (status.ready_switch_on() | status.switched_on()) {
			// move to "operation enabled"
			control.set_quick_stop(true);
			control.set_enable_voltage(true);
			control.set_switch_on(true);
			control.set_enable_operation(true);
		}
		// state "fault"
		else if status.fault() && ! status.switched_on() {
			// move to "switch on disabled"
			control.set_reset_fault(true);
		}
		// we are in the state we wanted !
		else if status.operation_enabled() {
			return;
		}
		group.set(controlword, control);
		
		println!("switch on {} {}  {:x}", status, control, u16::from(control));
	}
}
