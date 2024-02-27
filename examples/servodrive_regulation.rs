use std::error::Error;
use core::time::Duration;
use futures_concurrency::future::Race;
use etherage::{
    EthernetSocket, Master,
    Slave, SlaveAddress, CommunicationState,
    data::Field,
    sdo::{self, Sdo, OperationMode},
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
                let offset_torque    = pdo.push(sdo::cia402::offset::torque);
        let mut channel = slave.channel(sdo::sync_manager.logical_read(), 0x1c00 .. 0x2000);
            let mut pdo = channel.push(sdo::Pdo::with_capacity(0x1a00, false, 10));
                let statusword       = pdo.push(sdo::cia402::statusword);
                let error            = pdo.push(sdo::cia402::error);
                let current_position = pdo.push(sdo::cia402::current::position);
    drop(slave);
    let group = master.group(&mapping);

    let kpp = Sdo::<f32>::sub(0x2012, 1, 0);   // position propotional gain
    let kvp = Sdo::<f32>::sub(0x2012, 5, 0);   // velocity propotional gain
    let kvi = Sdo::<f32>::sub(0x2012, 6, 0);   // velocity integral gain

    let mut slave = Slave::new(&master, SlaveAddress::AutoIncremented(0)).await?;
    slave.switch(CommunicationState::Init).await?;
    slave.set_address(1).await?;
    slave.init_coe().await?;
    slave.switch(CommunicationState::PreOperational).await?;

    group.configure(&slave).await?;
    
    slave.switch(CommunicationState::SafeOperational).await?;
    slave.switch(CommunicationState::Operational).await?;

    let cycle = tokio::sync::Notify::new();
    let period = 1e-3; // refreshing period (s)

    (
        async {
            let mut period = tokio::time::interval(Duration::from_secs_f32(period));
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
                initial as f32
            };

            println!("switch on");
            switch_on(statusword, controlword, error, &group, &cycle).await;

            let position_frequency = 0.05;  // sine frequency (Hz)
            let position_amplitude = 200e3; // sine amplitude (coder pulses)
            let torque_frequency = 4.;  // sine frequency (Hz)
            let torque_amplitude = 400.; // sine amplitude (rated/1000)
            let mut t = 0.;

			println!("move");
			loop {

                cycle.notified().await;
                let mut group = group.data().await;

                group.set(target_position, (initial + (t*position_frequency*2.*std::f32::consts::PI).sin() * position_amplitude) as _);
                group.set(offset_torque,   ((t*torque_frequency*2.*std::f32::consts::PI).sin() * torque_amplitude) as _);

                t += period;
            }
        },
        async {
            use bilge::prelude::u2;
            let priority = u2::new(0);

            println!("initial {} {} {}",
                slave.coe().await.sdo_read(&kpp, priority).await.unwrap(),
                slave.coe().await.sdo_read(&kvp, priority).await.unwrap(),
                slave.coe().await.sdo_read(&kvi, priority).await.unwrap(),
                );

            let regulator_period = 10.;

            loop {
                println!("low");
                slave.coe().await.sdo_write(&kvp, priority, 0.5).await.unwrap();
                slave.coe().await.sdo_write(&kvp, priority, 0.6).await.unwrap();
                slave.coe().await.sdo_write(&kvi, priority, 0.1).await.unwrap();
                tokio::time::sleep(Duration::from_secs_f32(regulator_period)).await;

                println!("med");
                slave.coe().await.sdo_write(&kvp, priority, 1.).await.unwrap();
                slave.coe().await.sdo_write(&kvp, priority, 2.).await.unwrap();
                slave.coe().await.sdo_write(&kvi, priority, 0.1).await.unwrap();
                tokio::time::sleep(Duration::from_secs_f32(regulator_period)).await;

                println!("high");
                slave.coe().await.sdo_write(&kvp, priority, 4.).await.unwrap();
                slave.coe().await.sdo_write(&kvp, priority, 6.).await.unwrap();
                slave.coe().await.sdo_write(&kvi, priority, 1.).await.unwrap();
                tokio::time::sleep(Duration::from_secs_f32(regulator_period)).await;
            }
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
