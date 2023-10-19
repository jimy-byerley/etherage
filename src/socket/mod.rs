/*!
    This module provide the trait [EthercatSocket], and several implementors allowing to use different physical layers for ethercat communication.
    
    - UDP socket allows to run multiple master, one ethercat segment each, on the same ethernet network (and same machine ethernet port). But exposes the ethercat network to possible delays due to ethernet packet collisions.
    - Raw socket allows one only master with one only ethercat segment on the ethernet network. It ensure no communication delay with an ethercat segment. 
    
    Both socket types allows the use of the same master ethernet port for other ethernet protocols such as normal internet operations.
    
    | socket type |  allowed masters on network  |  allowed EC segments on network |  possible jitter |  other protocols allowed on same network |
    |-------------|----------------------------------------|-------------------------------------------|------------------|------------------------------------------|
    | [EthernetSocket] | 1                                 | 1                                         | none             | all non-ethercat protocols               |
    | [UdpSocket] | 2^32                                    | 2^32                                       | depend on trafic | all                                      |
*/

mod udp;
mod ethernet;

pub use udp::UdpSocket;
pub use ethernet::EthernetSocket;
use core::future::Future;
use core::task::{Poll, Context};

use std::io;

/**
    trait implementing the ethercat frame encapsulation into some medium
    
    This allows to send or receive ethercat frames over any network, but according to ETG 1000.4, only Ethernet and UDP are officially supported
*/
pub trait EthercatSocket {
    /** 
        receive an ethercat frame into the given buffer (starting from ethercat header) 
        
        The buffer should be big enough for the data to receive. No indication of the data size is provided by the socket. Returns the number of bytes read.
    
        The implementor is responsible from assembling the whole packet, and hiding the details of socket-specific headers, footers, checks, fragmentation ...
    */
    fn poll_receive(&self, cx: &mut Context<'_>, data: &mut [u8]) -> Poll<io::Result<usize>>;
    
    /** 
        send an ethercat frame contained in the given buffer.
        
        The whole buffer will be sent, the user has to tail it to the exact data size to send.
    
        the buffer passed must contain the data with the ethercat header. 
        The implentor of this trait is responsible of encapsulating the data into the specific socket by adding the necessary specific headers, footers, checks, fragmentation ...
    */
    fn poll_send(&self, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<()>>;
    
    /// maximum frame size tolerated for sending by this socket
    fn max_frame(&self) -> usize;
}
