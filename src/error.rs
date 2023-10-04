//! definition of the general ethercat error type

use std::sync::Arc;
use core::fmt;
use crate::data::PackingError;

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
        <Self as fmt::Debug>::fmt(self, f)
        // TODO: make a more readable error in this Display impl
    }
}
impl<T: fmt::Debug> std::error::Error for EthercatError<T> {}


impl<T> From<std::io::Error> for EthercatError<T> {
    fn from(src: std::io::Error) -> Self {
        EthercatError::Io(Arc::new(src))
    }
}

impl<T> From<PackingError> for EthercatError<T> {
    fn from(src: PackingError) -> Self {
        EthercatError::Protocol(match src {
            PackingError::BadSize(_, text) => text,
            PackingError::BadAlignment(_, text) => text,
            PackingError::InvalidValue(text) => text,
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
