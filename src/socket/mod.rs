mod udp;
mod ethernet;

pub use udp::UdpSocket;
pub use ethernet::EthernetSocket;

use std::io;

/**
    trait implementing the ethercat frame encapsulation into some medium
    
    This allows to send or receive ethercat frames over any network, but according to ETG 1000.4, only Ethernet and UDP are officially supported
*/
pub trait EthercatSocket {
    // receive an ethercat frame into the given buffer
    fn receive<'a>(&self, data: &'a mut [u8]) -> &'a mut [u8];
    // send an ethercat frame
    // the buffer passed must contain the data without the ethercat header, the header containing the frame length and type are added by this function
    fn send(&self, data: &[u8]);
}
