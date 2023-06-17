/*!
    Etherage is a crate implementing an ethercat master, with an API as close as possible to the concepts of the ethercat protocol.
    
    ## It mainly features
    
    - [Master] for protocol-safe and memory-safe access to the functions of the master
    - [Slave] for protocol-safe and memory-safe access to the functions of slaves
    - [RawMaster] and other structures based on it for memory-safe but protocol-unsafe access to lower level features of the protocol
    
    ## Complete feature list
    
    - [x] master over different sockets
        + [x] raw ethernet
        + [x] UDP
    - [ ] minimalistic features
        - [x] PDU commands
        - [x] registers access
        - [ ] slave information access
    - [x] mailbox
        + generic messaging
        + [x] COE
            - [x] SDO read/write
            - [ ] PDO read/write
            - [ ] informations
            - [x] tools for mapping
        + [ ] EOE
        + [ ] FOE
    - [ ] distributed clock
        + [ ] static drift
        + [ ] dynamic drift
    - optimization features
        + [x] multiple PDUs per ethercat frame (speed up and compress transmissions)
        + [x] tasks for different slaves or for same slave are parallelized whenever possible
        + [x] no dynamic allocation in transmission and realtime functions
*/

pub mod data;
#[allow(non_upper_case_globals)] 
#[allow(unused)]
pub mod registers;

pub mod socket;
pub mod rawmaster;
pub mod mailbox;
pub mod sdo;
pub mod can;
pub mod master;
pub mod slave;
pub mod mapping;


pub use crate::data::{PduData, Field, BitField};
pub use crate::sdo::Sdo;
pub use crate::socket::*;
pub use crate::rawmaster::*;
pub use crate::master::*;
pub use crate::slave::*;
pub use crate::mapping::{Mapping, Group};


use std::sync::Arc;

/// general object reporting an unexpected result regarding ethercat communication
#[derive(Clone, Debug)]
pub enum EthercatError<T> {
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
    /// these errors can generally be handled and fixed by retrying the operation or using the master differently
    Master(&'static str),
    
    /// error detected by the master in the ethercat communication
    ///
    /// these errors can generally not be fixed and the whole communication has to be restarted
    Protocol(&'static str),
}
