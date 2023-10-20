use std::{
    sync::Arc,
    error::Error,
    };
use core::time::Duration;
use futures_concurrency::future::Join;
use etherage::{
    EthernetSocket, RawMaster,
    Slave, SlaveAddress, CommunicationState,
    data::Field,
    sdo::{self, SyncDirection, OperationMode},
    mapping::{self, Mapping, Group},
    registers,
    };

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let master = RawMaster::new(EthernetSocket::new("eno1")?);
    
    println!("create mapping");
    let config = mapping::Config::default();
    let mapping = Mapping::new(&config);
    let mut slave = mapping.slave(1);
        let _statuscom = slave.register(SyncDirection::Read, registers::al::status);
        let mut channel = slave.channel(sdo::sync_manager::logical_write);
            let mut pdo              = channel.push(sdo::Pdo::with_capacity(0x1600, false, 10));
                let controlword      = pdo.push(sdo::cia402::controlword);
                let target_mode      = pdo.push(sdo::cia402::target::mode);
                let target_position  = pdo.push(sdo::cia402::target::position);
                let _target_velocity = pdo.push(sdo::cia402::target::velocity);
                let _target_torque = pdo.push(sdo::cia402::target::torque);
        let mut channel = slave.channel(sdo::sync_manager::logical_read);
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1a00, false, 10));
                let statusword       = pdo.push(sdo::cia402::statusword);
                let error            = pdo.push(sdo::cia402::error);
                let _current_mode  = pdo.push(sdo::cia402::current::mode);
                let current_position = pdo.push(sdo::cia402::current::position);
                let _current_velocity = pdo.push(sdo::cia402::current::velocity);
                let _current_torque  = pdo.push(sdo::cia402::current::torque);
    drop(slave);
    println!("done {:#?}", config);
    
    let allocator = mapping::Allocator::new();
    let group = allocator.group(&master, &mapping);
    
    println!("group {:#?}", group);
    println!("fields {:#?}", (statusword, controlword));

    let mut slave = Slave::raw(master.clone(), SlaveAddress::AutoIncremented(0));
    slave.switch(CommunicationState::Init).await?;
    slave.set_address(1).await?;
    slave.init_mailbox().await?;
    slave.init_coe().await;
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

            use bilge::prelude::u2;
            println!("power: {:?}", slave.coe().await.sdo_read(&sdo::error, u2::new(0)).await);

            println!("switch on");
            switch_on(statusword, controlword, error, &group, &cycle).await;

			let velocity = 3_000_000;
			let increment = 100_000_000;

			println!("move forward");
			loop {
                cycle.notified().await;
                let mut group = group.data().await;

                let position = group.get(current_position);
                if position >= initial + increment {break}
                group.set(target_position, position + velocity);
                println!("    {}", position);
            }

			println!("move backward");
            loop {
                cycle.notified().await;
                let mut group = group.data().await;

                let position = group.get(current_position);
                if position <= initial {break}
                group.set(target_position, position - velocity);
                println!("    {}", position);
            }

            println!("done");
        },
    ).join().await;

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
