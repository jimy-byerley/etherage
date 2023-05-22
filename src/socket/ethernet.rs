use std::os::unix::io::{AsRawFd, RawFd};
use std::io;
use super::EthercatSocket;


/**
    Raw socket allowing direct ethercat com, but only one segment on the ethernet network

    Raw sockets are not implemented in std::net, so here is an implementation found in `smoltcp` and `ethercrab`.
    This implementation is unix-specific
*/
#[derive(Debug)]
pub struct EthernetSocket {
    protocol: libc::c_ushort,
    lower: libc::c_int,
    ifreq: ifreq,
}

impl EthernetSocket {
    pub fn new(interface: &str, protocol: u16) -> io::Result<Self> {
        let protocol: u16 = 0x88a4;  // ethernet type: ethercat

        // create
        let lower = unsafe {
            let lower = libc::socket(
                // Ethernet II frames
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK,
                protocol.to_be() as i32,
            );
            if lower == -1 {
                return Err(io::Error::last_os_error());
            }
            lower
        };

        let mut new = EthernetSocket {
            protocol,
            lower,
            ifreq: ifreq_for(interface),
        };

        // bind
        let sockaddr = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: new.protocol.to_be() as u16,
            sll_ifindex: ifreq_ioctl(new.lower, &mut new.ifreq, libc::SIOCGIFINDEX)?,
            sll_hatype: 1,
            sll_pkttype: 0,
            sll_halen: 6,
            sll_addr: [0; 8],
        };

        unsafe {
            #[allow(trivial_casts)]
            let res = libc::bind(
                new.lower,
                &sockaddr as *const libc::sockaddr_ll as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
            );
            if res == -1 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(new)
    }
}

impl Drop for EthernetSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.lower);
        }
    }
}

impl AsRawFd for EthernetSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.lower
    }
}

impl EthercatSocket for EthernetSocket {
    fn receive<'a>(&self, data: &'a mut [u8]) -> &'a mut [u8] {
        let len = unsafe {
            libc::read(
                self.as_raw_fd(),
                data.as_mut_ptr() as *mut libc::c_void,
                data.len(),
            )
        };
        if len == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(len as usize)
        }
        data[..len]
    }
    fn send(&self, data: &[u8]) {
        let len = unsafe {
            libc::write(
                self.as_raw_fd(),
                data.as_ptr() as *mut libc::c_void,
                data.len(),
            )
        };
        if len == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(len as usize)
        }
    }
}


// intermediate C-like structures and functions

#[repr(C)]
#[derive(Debug)]
struct ifreq {
    ifr_name: [libc::c_char; libc::IF_NAMESIZE],
    ifr_data: libc::c_int, /* ifr_ifindex or ifr_mtu */
}

fn ifreq_ioctl(
    lower: libc::c_int,
    ifreq: &mut ifreq,
    cmd: libc::c_ulong,
) -> io::Result<libc::c_int> {
    unsafe {
        #[allow(trivial_casts)]
        let res = libc::ioctl(lower, cmd, ifreq as *mut ifreq);

        if res == -1 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(ifreq.ifr_data)
}

fn ifreq_for(name: &str) -> ifreq {
    let mut ifreq = ifreq {
        ifr_name: [0; libc::IF_NAMESIZE],
        ifr_data: 0,
    };
    for (i, byte) in name.as_bytes().iter().enumerate() {
        ifreq.ifr_name[i] = *byte as libc::c_char
    }
    ifreq
}
