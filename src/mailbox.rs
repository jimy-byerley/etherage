//! implementation of communication with a slave's mailbox

use crate::{
    error::{EthercatError, EthercatResult},
    rawmaster::{RawMaster, PduCommand, SlaveAddress},
    registers,
    data::{self, PduData, Cursor},
    };
use core::ops::Range;
use std::sync::Arc;
use bilge::prelude::*;
use futures_concurrency::future::Join;


/// arbitrary maximum size for a mailbox buffer
/// the user may select a smaller size, especially if the mailbox is used at the same time as buffered sync managers
const MAILBOX_MAX_SIZE: usize = 1024;

/**
    implementation of communication with a slave's mailbox

    The mailbox is a mean for ethercat slaves to implement non-minimalistic ethercat features, and features that do not fit in registers (because they rely on imperative designs, or variable size data ...).

    The mailbox is an optional feature of an ethercat slave.

    Following ETG.1000.4 6.7.1 it is using the first 2 sync managers in handshake mode. Buffered mode is not meant for mailbox.
*/
pub struct Mailbox {
    master: Arc<RawMaster>,
    slave: u16,
    read: Direction,
    write: Direction,
}
struct Direction {
    count: u8,
    address: u16,
    max: usize,
}

impl Mailbox {
    /**
        configure the mailbox on the slave, using the given `read` and `write` memory areas as mailbox buffers

        `slave` is the slave's fixed address, no implementation is made for mailbox with topological addresses
    */
    pub async fn new(master: Arc<RawMaster>, slave: u16, write: Range<u16>, read: Range<u16>) -> EthercatResult<Mailbox> {
        // check that there is not previous error
        if master.fprd(slave, registers::al::response).await.one()?.error() {
            panic!("mailbox error before init: {:?}", master.fprd(slave, registers::al::error).await.one());
        }

        // configure sync manager
        let configured = (
            master.fpwr(slave, registers::sync_manager::interface.mailbox_write(), {
                let mut config = registers::SyncManagerChannel::default();
                config.set_address(write.start);
                config.set_length(write.end - write.start);
                config.set_mode(registers::SyncMode::Mailbox);
                config.set_direction(registers::SyncDirection::Write);
                config.set_dls_user_event(true);
                config.set_ec_event(true);
                config.set_enable(true);
                config
            }),

            master.fpwr(slave, registers::sync_manager::interface.mailbox_read(), {
                let mut config = registers::SyncManagerChannel::default();
                config.set_address(read.start);
                config.set_length(read.end - read.start);
                config.set_mode(registers::SyncMode::Mailbox);
                config.set_direction(registers::SyncDirection::Read);
                config.set_dls_user_event(true);
                config.set_ec_event(true);
                config.set_enable(true);
                config
            }),
        ).join().await;
        if configured.0.one().is_err() || configured.1.one().is_err()
            {return Err(EthercatError::Master("failed to configure mailbox sync managers"))}

        assert!(usize::from(read.end - read.start) < MAILBOX_MAX_SIZE);
        assert!(usize::from(write.end - write.start) < MAILBOX_MAX_SIZE);

        Ok(Self {
            master,
            slave,
            read: Direction{
                count: 0,
                address: read.start,
                max: usize::from(read.end - read.start),
                },
            write: Direction{
                count: 0,
                address: write.start,
                max: usize::from(write.end - write.start),
                },
        })
    }
    pub async fn poll(&self) -> bool {todo!()}
    pub async fn available(&self) -> usize {todo!()}

    /**
        read the frame currently in the mailbox, wait for it if not already present

        `data` is the buffer to fill with the mailbox, only the first bytes corresponding to the current buffer size on the slave will be read

        return the slice of data received.

        - 0 is lowest priority, 3 is highest
    */
    pub async fn read<'a>(&mut self, ty: MailboxType, data: &'a mut [u8]) -> EthercatResult<&'a [u8], MailboxError> {
        let mailbox_control = registers::sync_manager::interface.mailbox_read();
        let mut allocated = [0; MAILBOX_MAX_SIZE];

        self.read.count = (self.read.count % 7) + 1;

        // wait for data
        let mut state: registers::SyncManagerChannel = loop {
            let state= self.master.fprd(self.slave, mailbox_control).await;
            if state.answers == 1 {
                let value = state.value()?;
                if value.mailbox_full() {
                    break value }
            }
        };
        // the destination data is expected to be big enough for the data, so we will read only this data size
        let range = .. allocated.len()
                        .min(data.len() + MailboxHeader::packed_size())
                        .min(self.read.max);
        let buffer = &mut allocated[range];
        // read the mailbox content
        loop {
            // exit loop if ok
            if self.master.pdu(PduCommand::FPRD, SlaveAddress::Fixed(self.slave), self.read.address.into(), buffer, false).await == 1 { break }

            // otherwise... trigger repeat
            state.set_repeat(true);
            while self.master.fpwr(self.slave, mailbox_control, state).await.answers == 0  {}
            // wait for repeated data to be available
            loop {
                let state = self.master.fprd(self.slave, mailbox_control).await;
                if state.answers == 0 || ! state.value()?.repeat_ack()  {continue}
                break
            }
        }
        let mut frame = Cursor::new(buffer.as_mut());
        let mut received = Cursor::new(data);
        let header = frame.unpack::<MailboxHeader>()
            .map_err(|_| EthercatError::Protocol("unable to unpack mailbox header"))?;
        if header.ty() == MailboxType::Exception {
            let error = frame.unpack::<MailboxErrorFrame>()
                .map_err(|_| EthercatError::Protocol("unable to unpack received mailbox error"))?;
            return Err(EthercatError::Slave(error.detail()))
        }
        if header.ty() != ty
            {return Err(EthercatError::Protocol("received unexpected mailbox frame type"))}
        if u8::from(header.count()) != self.read.count
            {return Err(EthercatError::Protocol("received mailbox frame has wrong counter"))}
        received.write(
            frame.read(header.length() as usize)
                .map_err(|_| EthercatError::Protocol("inconsistent mailbox size"))?
            ).map_err(|_| EthercatError::Master("read buffer is too small for mailbox data"))?;

        Ok(received.finish())
    }
    /**
        write the given frame in the mailbox, wait for it first if already busy

        - 0 is lowest priority, 3 is highest
    */
    pub async fn write(&mut self, ty: MailboxType, priority: u2, data: &[u8]) -> EthercatResult<(), MailboxError> {
        let mailbox_control = registers::sync_manager::interface.mailbox_write();
        let mut allocated = [0; MAILBOX_MAX_SIZE];
        let buffer = &mut allocated[.. self.write.max];

        self.write.count = (self.write.count % 7)+1;

        let mut frame = Cursor::new(buffer.as_mut());
        frame.pack(&MailboxHeader::new(
                data.len() as u16,
                u16::new(0),  // address of master
                u6::new(0),  // this value has no effect and is reserved for future use
                priority,
                ty,
                u3::new(self.write.count),
            )).unwrap();
        frame.write(data)
            .map_err(|_|  EthercatError::Master("data too big for mailbox buffer"))?;
        let _sent = frame.finish(); //TODO: Unused, is it wanted ?

        // wait for mailbox to be empty
        loop {
            let state = self.master.fprd(self.slave, mailbox_control).await;
            if state.answers == 1 {
                if ! state.value()?.mailbox_full()  {break}
            }
        }
        // write data
        // TODO: retry this solution with writing the last word instead of the last byte
        // we are forced to write the whole buffer (even if much bigger than data) because the slave will notice the data sent only if writing the complete buffer
        // and writing the last byte instead does not work trick it.
//         if mailbox_size - data.len() > 32 {
//             loop {
//                 // write beginning of buffer and last byte for slave triggering
//                 let (writing, end) = futures::join!(
//                     self.master.pdu(PduCommand::FPWR, self.slave, self.write.address, sent),
//                     // the mailbox processing is done once the last mailbox byte is written, so write the last byte alone
//                     self.master.fpwr(self.slave, Field::<u8>::simple(usize::from(self.write.end)-1), 0),
//                     );
//                 if end.answers == 1 && writing == 1
//                     {break}
//             }
//         }
//         else {
            // write the full buffer
            while self.master.pdu(PduCommand::FPWR, SlaveAddress::Fixed(self.slave), self.write.address.into(), buffer.as_mut(), false).await != 1
                {}
//         }
        Ok(())
    }
}

/// ETG 1000.4 table 29
#[bitsize(48)]
#[derive(TryFromBits, DebugBits, Copy, Clone)]
struct MailboxHeader {
    /// length of the mailbox service data following this header
    length: u16,
    /**
        - if a master is client: Station Address of the source
        - if a slave is client: Station Address of the destination
    */
    address: u16,
    /// reserved for future
    channel: u6,
    /// 0 is lowest priority, 3 is highest
    priority: u2,
    ty: MailboxType,
    /// Counter of the mailbox services (0 reserved, this should roll from 1 to 7 and overflow to 1 after 7)
    count: u3,
    reserved: u1,
}
data::bilge_pdudata!(MailboxHeader, u48);

/// ETG 1000.4 table 29
#[bitsize(4)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum MailboxType {
    Exception = 0x0,
    Ads = 0x1,
    Ethernet = 0x2,
    Can = 0x3,
    File = 0x4,
    Servo = 0x5,
    Specific = 0xf,
}

/// ETG 1000.4 table 30
#[bitsize(32)]
#[derive(TryFromBits, DebugBits, Copy, Clone, Eq, PartialEq)]
struct MailboxErrorFrame {
    ty: u16,
    detail: MailboxError,
}
data::bilge_pdudata!(MailboxErrorFrame, u32);

// ETG 1000.4 table 30
#[bitsize(16)]
#[derive(TryFromBits, Debug, Copy, Clone, Eq, PartialEq)]
pub enum MailboxError {
    /// Syntax of 6 octet Mailbox Header is wrong
    Syntax = 0x1,
    /// The Mailbox protocol is not supported
    UnsupportedProtocol = 0x2,
    /// Channel Field contains wrong value (a slave can ignore the channel field)
    InvalidChannel = 0x3,
    /// the service in the Mailbox protocol is not supported
    ServiceNotSupported = 0x4,
    /// The mailbox protocol header of the mailbox protocol is wrong (without the 6 octet mailbox header)
    InvalidHeader = 0x5,
    /// length of received mailbox data is too short for slave's expectations
    SizeTooShort = 0x6,
    /// Mailbox protocol cannot be processed because of limited ressources
    NoMoreMemory = 0x7,
    /// the length of data is inconsistent
    InvalidSize = 0x8,
    /// Mailbox service already in use
    ServiceInWork = 0x9,
}
