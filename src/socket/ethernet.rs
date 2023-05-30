use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use packed_struct::prelude::*;
use packed_struct::types::bits::ByteArray;
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
    pub fn new(interface: &str) -> io::Result<Self> {
        let protocol: u16 = 0x88a4;  // ethernet type: ethercat

        // create
        let lower = unsafe {
            let lower = libc::socket(
                // Ethernet II frames
                libc::AF_PACKET,
                libc::SOCK_RAW, // | libc::SOCK_NONBLOCK,
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
    fn receive(&self, data: &mut [u8]) -> io::Result<usize> {
        let mut packed = [0u8; 4096];
        let len = unsafe {
            libc::read(
                self.as_raw_fd(),
                packed.as_mut_ptr() as *mut libc::c_void,
                packed.len(),
            )
        };
        println!("received {:?}", &packed[..(len as usize)]);
        let header = EthernetHeader::unpack_from_slice(&packed[..<EthernetHeader as PackedStruct>::ByteArray::len()]).unwrap();
        if header.dst != [0x10, 0x10, 0x10, 0x10, 0x10, 0x10]
        || header.src != [0x12, 0x10, 0x10, 0x10, 0x10, 0x10]
        || header.ty != 0x88a4
            {}
        data.copy_from_slice(&packed[<EthernetHeader as PackedStruct>::ByteArray::len()..]);
        if len < 0 {
            Err(io::Error::last_os_error())
        }
        else {
            Ok(len as usize)
        }
    }
    fn send(&self, data: &[u8]) -> io::Result<()> {
        let mut packed = heapless::Vec::<u8, 4096>::new();
        let padding = [0; 60];
        packed.extend_from_slice(EthernetHeader {
            dst: [0x10, 0x10, 0x10, 0x10, 0x10, 0x10],
            src: [0x12, 0x10, 0x10, 0x10, 0x10, 0x10],
            // vlan is said to be optional and this is not present in most ethercat frames, so will not be used here
            // vlan: [0x81, 0x0, 0, 0],
            ty: 0x88a4,
            }.pack().unwrap().as_bytes_slice());
        packed.extend_from_slice(data);
        // TODO: checksum ?
        if packed.len() < padding.len() {
            packed.extend_from_slice(&padding[packed.len()..]);
        }
        let data = packed.as_slice();
        
        let len = unsafe {
            libc::write(
                self.as_raw_fd(),
                data.as_ptr() as *mut libc::c_void,
                data.len(),
            )
        };
        if len < 0 || (len as usize) != data.len() {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
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



#[derive(PackedStruct, Clone, Debug)]
#[packed_struct(size_bytes="14", bit_numbering = "lsb0", endian = "msb")]
struct EthernetHeader {
    #[packed_field(bytes="8:13")]  dst: [u8;6],
    #[packed_field(bytes="2:7")]  src: [u8;6],
    // vlan is said to be optional and this is not present in most ethercat frames, so will not be used here
//     #[packed_field(bytes="2:5")]  vlan: [u8;4],  
    #[packed_field(bytes="0:1")]  ty: u16,
}
#[derive(PackedStruct, Clone, Debug)]
#[packed_struct(size_bytes="4", bit_numbering = "lsb0", endian = "msb")]
struct EthernetFooter {
    #[packed_field(bytes="0:3")]  checksum: u32,
}
