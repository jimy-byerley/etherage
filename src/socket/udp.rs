use std::io;
use std::net::{SocketAddr, IpAddr, Ipv4Addr};
use super::EthercatSocket;

/**
    UDP socket with fixed port, allowing ethercat com through a regular switch
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
                ));
        Ok(Self {
            address,
            socket,
            filter_address: false,
        })
    }
    pub fn set_filter_address(&mut self, enable: bool) {
        self.filter_address = enable;
    }
}

impl EthercatSocket for UdpSocket {
    fn receive<'a>(&self, data: &'a mut [u8]) -> &'a mut [u8] {
        let (size, src) = self.socket.recv_from(data)?;
        // ignore wrong hosts
        if self.filter_address && SocketAddr::V4(self.address) != src {}
        data[..size]
    }
    fn send(&self, data: &[u8]) {
        self.socket.send_to(data, self.address)
    }
}
