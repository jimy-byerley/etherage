//! definition of the general ethercat error type

use std::{sync::Arc, any::TypeId};
use core::fmt;
use crate::{registers::AlError, can::{CanError, SdoAbortCode}, mailbox::MailboxError};
use chrono::{Datelike, Timelike};

/**
    general object reporting an unexpected result regarding ethercat communication

    Its variant are meant to help finding the cause responsible for the problem and how to deal with it.

    [Self::Slave] variant should not be used without an appropriate type for `T`, `T` depend on the operation the slave reports for, and is usually an error code, or an enum.
*/
#[derive(Clone, Debug)]
pub enum EthercatError<T=()> {
    /// error caused by communication support
    ///
    /// these errors are exterior to this library
    Io(Arc<std::io::Error>),

    /// error reported by a slave, its type depend on the operation returning this error
    ///
    /// these errors can generally be handled and fixed by retrying the operation or reconfiguring the slave
    Slave(T),

    /// error reported by the master
    ///
    /// these errors can generally be handled and fixed by retrying the operation or using the master differently when the issue is in the user code
    Master(&'static str),

    /// error detected by the master in the ethercat communication
    ///
    /// these errors can generally not be fixed and the whole communication has to be restarted
    Protocol(&'static str),

    /// error is due to too much time elapsed, but does not compromise the communication
    ///
    /// these errors are generally contextual and the operation can be retried.
    Timeout(&'static str),
}

/// convenient alias to simplify return annotations
pub type EthercatResult<T=(), E=()> = core::result::Result<T, EthercatError<E>>;

impl<T: fmt::Debug> fmt::Display for EthercatError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {

        //Get timestamp of these error
        let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
        let timestamp : String = String::from(format!("{}/{:02}/{:02} {} {:02}:{:02}{:02}:{:06} - ",
        now.year(), now.month(), now.day(), now.weekday(),
        now.hour(), now.minute(), now.second(), now.timestamp_subsec_micros()  ));

        //Get source and description for this error
        let (src, mut msg) : (String, String) = match self {
            Self::Io(value) => { (String::from("Io"), value.to_string()) },
            Self::Master(value)   => { (String::from("Master"), value.to_string()) },
            Self::Protocol(value) => { (String::from("Protocol"), value.to_string()) },
            Self::Timeout(value)  => { (String::from("Timeout"), value.to_string()) },
            Self::Slave(value) => { (String::from("Slave"), "TODO value".to_string()) } // value.to_string() }
                // let item0: String = String::from("Slave");
                // let mut item1 : String;
                // if TypeId::of::<T>() == TypeId::of::<CanError>() {
                //     let temp2: &CanError = unsafe { std::mem::transmute::<&T, &CanError>(value) };
                //     item1 = match temp2 {
                //         CanError::Sdo(e) => {
                //             let err_code : &SdoAbortCode = unsafe { std::mem::transmute::<&T,&SdoAbortCode>(value.clone()) };
                //             String::from(format!("Sdo: {}", *err_code as i32))
                //         },
                //         CanError::Mailbox(e) => {
                //             let err_code : &MailboxError = unsafe { std::mem::transmute::<&T,&MailboxError> (value.clone()) };
                //             String::from(format!("Mailbox: {}", *err_code as i16))
                //          },
                //     }
                // }
                // else if TypeId::of::<T>() == TypeId::of::<AlError>() {
                //     //let err_code: &AlError = unsafe { std::mem::transmute::<&T,&AlError>(value.clone()) };
                //     //item1 = String::from(format!("AlError code: {}", *err_code as i16));
                //     item1 = String::from(format!("{}", unsafe { std::mem::transmute::<&T,&AlError>(value.clone()) }));
                // }
                // (item0, item1)
            //},
        };

        f.debug_struct("EthercatError")
            .field("timestamp", &timestamp)
            .field("source", &src)
            .field("message", &msg)
            .finish()
    }
}

impl<T: fmt::Debug> std::error::Error for EthercatError<T> {}

impl<T> From<std::io::Error> for EthercatError<T> {
    fn from(src: std::io::Error) -> Self {
        EthercatError::Io(Arc::new(src))
    }
}

impl<T> From<crate::data::PackingError> for EthercatError<T> {
    fn from(src: crate::data::PackingError) -> Self {
        EthercatError::Protocol(match src {
            crate::data::PackingError::BadSize(_, text) => text,
            crate::data::PackingError::BadAlignment(_, text) => text,
            crate::data::PackingError::InvalidValue(text) => text,
        })
    }
}

// because rust doesn't allow specialization and already implements `From<T> for T`, we cannot write smart conversions for generic EthercatError<T>, so these are manual conversion methods
impl<E> EthercatError<E> {
    /// convert the error if the slave specific error type allows it
    pub fn into<F>(self) -> EthercatError<F>
    where F: From<E> {
        self.map(|e| F::from(e))
    }
    /// convert the error with a callback handling the case of slave-specific error
    pub fn map<F,T>(self, callback: F) -> EthercatError<T>
    where F: Fn(E) -> T
    {
        match self {
            EthercatError::Slave(value) => EthercatError::Slave(callback(value)),
            EthercatError::Io(e) => EthercatError::Io(e),
            EthercatError::Master(message) => EthercatError::Master(message),
            EthercatError::Protocol(message) => EthercatError::Protocol(message),
            EthercatError::Timeout(message) => EthercatError::Timeout(message),
        }
    }
}
impl EthercatError<()> {
    /// convert an error with no slave-specific type into an error with
    pub fn upgrade<F>(self) -> EthercatError<F> {
        self.map(|_|  unimplemented!("an ethercat error with not slave-specific error type cannot report a slave error"))
    }
}
