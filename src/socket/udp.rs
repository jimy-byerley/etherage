use std::io;
use std::net::{SocketAddr, IpAddr, Ipv4Addr};
use std::os::fd::{AsRawFd, RawFd};
use super::EthercatSocket;

/**
    UDP socket with fixed port, allowing ethercat com through a regular switch
    
    Ethercat masters and slaves are IP-addressed, so there can be any number of masters and slaves on the network.
*/
pub struct UdpSocket {
    socket: std::net::UdpSocket,
    address: SocketAddr,
    
    filter_address: bool,
}

impl UdpSocket {
    /// according to ETG.1000.4 only IPv4 is supported, and port is fixed, hence this function only requires the host address
    pub fn new(segment: Ipv4Addr) -> io::Result<Self> {
        let port = 0x88a4;
        let address = SocketAddr::new(
                IpAddr::V4(segment),
                port,
                );
        let socket = std::net::UdpSocket::bind(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
                ))?;
        socket.set_nonblocking(false)?;
        Ok(Self {
            address,
            socket,
            filter_address: true,
        })
    }
    /// if enabled, the incoming packets coming from wrong host will be ignored
    pub fn set_filter_address(&mut self, enable: bool) {
        self.filter_address = enable;
    }
}

impl EthercatSocket for UdpSocket {
    fn receive(&self, data: &mut [u8]) -> io::Result<usize> {
        let (size, src) = self.socket.recv_from(data)?;
        // ignore wrong hosts if needed
        if self.filter_address && self.address != src {}
        Ok(size)
    }
    fn send(&self, data: &[u8]) -> io::Result<()> {
        self.socket.send_to(data, self.address)?;
        Ok(())
    }
    fn max_frame(&self) -> usize  { 
        1500 // max ethernet payload in 802.3 is 1500 bytes
        - 20 // IP header
        - 8 // UDP header
    }
}

impl AsRawFd for UdpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}
