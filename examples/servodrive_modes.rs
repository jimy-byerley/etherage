use std::error::Error;
use core::time::Duration;
use futures_concurrency::future::Race;
use etherage::{
    EthernetSocket, Master,
    Slave, SlaveAddress, CommunicationState,
    data::Field,
    sdo::{self, OperationMode},
    mapping::{self, Mapping, Group},
    registers::{self, SyncDirection},
    };

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = Master::new(EthernetSocket::new("eno1")?);
    
    let config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slave = mapping.slave(1);
        let _statuscom = slave.register(SyncDirection::Read, registers::al::status);
        let mut channel = slave.channel(sdo::sync_manager.logical_write(), 0x1800 .. 0x1c00);
            let mut pdo              = channel.push(sdo::Pdo::with_capacity(0x1600, false, 10));
                let controlword      = pdo.push(sdo::cia402::controlword);
                let target_mode      = pdo.push(sdo::cia402::target::mode);
                let target_position  = pdo.push(sdo::cia402::target::position);
                let target_velocity  = pdo.push(sdo::cia402::target::velocity);
                let target_torque    = pdo.push(sdo::cia402::target::torque);
        let mut channel = slave.channel(sdo::sync_manager.logical_read(), 0x1c00 .. 0x2000);
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1a00, false, 10));
                let statusword       = pdo.push(sdo::cia402::statusword);
                let error            = pdo.push(sdo::cia402::error);
                let current_position = pdo.push(sdo::cia402::current::position);
    drop(slave);
    let group = master.group(&mapping);

    let mut slave = Slave::new(&master, SlaveAddress::AutoIncremented(0)).await?;
    slave.switch(CommunicationState::Init).await?;
    slave.set_address(1).await?;
    slave.init_coe().await?;
    slave.switch(CommunicationState::PreOperational).await?;

    group.configure(&slave).await?;
    
    slave.switch(CommunicationState::SafeOperational).await?;
    slave.switch(CommunicationState::Operational).await?;

    let cycle = tokio::sync::Notify::new();

    (
        async {
            let mut period = tokio::time::interval(Duration::from_millis(1));
            loop {
                group.data().await.exchange().await;
                cycle.notify_waiters();
                period.tick().await;
            }
        },
        async {
            let initial = {
                let mut group = group.data().await;

                let initial = group.get(current_position);
                group.set(target_mode, OperationMode::SynchronousPosition);
                group.set(target_position, initial);

                println!("error {:x}", group.get(error));
                println!("position  {}", initial);
                initial
            };

            println!("switch on");
            switch_on(statusword, controlword, error, &group, &cycle).await;

            // coder with 2^18 pulses/rotation
// 			let increment = 3_000_000;
// 			let course = 100_000_000;
            // coder with less
			let increment = 1_000;
			let course = 100_000;


			println!("move forward");
            group.data().await.set(target_mode, OperationMode::SynchronousPosition);
			loop {
                cycle.notified().await;
                let mut group = group.data().await;

                let position = group.get(current_position);
                if position >= initial + course {break}
                group.set(target_position, position + increment);
                print!("    {}\r", position);
            }
            {
                let data = group.data().await;
                println!("{}  {:x}", data.get(statusword), data.get(error));
            }

            println!("mode velocity");
            {
                let mut data = group.data().await;
                data.set(target_mode, OperationMode::SynchronousVelocity);
                data.set(target_velocity, 0);
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
            {
                let data = group.data().await;
                println!("{}  {:x}", data.get(statusword), data.get(error));
            }

            println!("mode force");
            {
                let mut data = group.data().await;
                data.set(target_mode, OperationMode::SynchronousTorque);
                data.set(target_torque, 0);
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
            {
                let data = group.data().await;
                println!("{}  {:x}", data.get(statusword), data.get(error));
            }

			println!("move backward");
            group.data().await.set(target_mode, OperationMode::SynchronousPosition);
            loop {
                cycle.notified().await;
                let mut group = group.data().await;

                let position = group.get(current_position);
                if position <= initial {break}
                group.set(target_position, position - increment);
                print!("    {}\r", position);
            }
            {
                let data = group.data().await;
                println!("{}  {:x}", data.get(statusword), data.get(error));
            }

            println!("done");
        },
    ).race().await;

    Ok(())
}



pub async fn switch_on(
		statusword: Field<sdo::StatusWord>,
		controlword: Field<sdo::ControlWord>,
		error: Field<u16>,
		group: &Group<'_>,
		cycle: &tokio::sync::Notify,
		) {
	loop {
        {
            let mut group = group.data().await;
            let status = group.get(statusword);
            let mut control = sdo::ControlWord::default();

            // state "not ready to switch on"
            // we are in the state we wanted !
            if status.operation_enabled() {
                return;
            }
            // state "fault"
            else if status.fault() && ! status.switched_on() {
                // move to "switch on disabled"
                control.set_reset_fault(true);
            }
            else if ! status.quick_stop() {
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
            group.set(controlword, control);
            println!("switch on {} {}  {:#x}", status, control, group.get(error));
        }

		cycle.notified().await;
	}
}
