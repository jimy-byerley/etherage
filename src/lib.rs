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
        + [ ] EOE
        + [ ] FOE
    - [ ] distributed clock
        + [ ] static drift
        + [ ] dynamic drift
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
// pub mod slave;


pub use crate::data::{PduData, Field, BitField};
pub use crate::sdo::Sdo;
pub use crate::socket::*;
pub use crate::rawmaster::*;
pub use crate::master::*;
// pub use crate::slave::*;
