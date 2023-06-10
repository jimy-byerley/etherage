use std::io::{self, Cursor, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use packed_struct::prelude::*;
use packed_struct::types::bits::ByteArray;
use super::EthercatSocket;


/**
    Raw socket allowing direct ethercat com, but only one segment on the ethernet network

    Raw sockets are not implemented in [std::net], so here is an implementation found in `smoltcp` and `ethercrab`.
    This implementation is unix-specific
*/
#[derive(Debug)]
pub struct EthernetSocket {
    protocol: libc::c_ushort,
    lower: libc::c_int,
    ifreq: ifreq,
    header: EthernetHeader,
    filter_address: bool,
}

/// biggest possible ethercat frame allowed by the protocol
/// ethernet header + ethercat header + 2^11
const MAX_ETHERNET_FRAME: usize = 2064;

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
            header: EthernetHeader {
                dst: [0x10, 0x10, 0x10, 0x10, 0x10, 0x10],
                src: [0x12, 0x10, 0x10, 0x10, 0x10, 0x10],
                // vlan is said to be optional and this is not present in most ethercat frames, so will not be used here
                // vlan: [0x81, 0x0, 0, 0],
                protocol,
                },
            filter_address: true,
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
    /// if enabled, the incoming packets with wrong src&dst header will be ignored
    pub fn set_filter_address(&mut self, enable: bool) {
        self.filter_address = enable;
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
        // the maximum ethernet frame used in ethercat is reasonably small so we can allocate the maximum on the stack
        let mut packed = [0u8; MAX_ETHERNET_FRAME];
        loop {
            let len = unsafe {
                libc::read(
                    self.as_raw_fd(),
                    packed.as_mut_ptr() as *mut libc::c_void,
                    packed.len(),
                )
            };
            if len < 0
                 {break Err(io::Error::last_os_error())}
            if len == 0
                {continue}
                 
            let frame = EthernetFrame::unpack(&packed[.. (len as usize)]);
            if frame.header != self.header
                {continue}
            data[.. frame.data.len()].copy_from_slice(frame.data);
            
            // extract content
            break {Ok(frame.data.len() as usize)}
        }
    }
    fn send(&self, data: &[u8]) -> io::Result<()> {
        // the maximum ethernet frame used in ethercat is reasonably small so we can allocate the maximum on the stack
        let mut packed = [0u8; MAX_ETHERNET_FRAME];
        let packet = EthernetFrame {header: self.header.clone(), data};
        packet.pack(&mut packed);
        let data = &packed[.. packet.size()];
        
        let len = unsafe {
            libc::write(
                self.as_raw_fd(),
                data.as_ptr() as *mut libc::c_void,
                data.len(),
            )
        };
        if len < 0 
            {Err(io::Error::last_os_error())} 
        else 
            {Ok(())}
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



/// an ethercat frame in its unpacked form
/// an ethercat frame has a variable size du to its data content, this is why the data is not owned here but meant to reference some user buffer
#[derive(Debug)]
struct EthernetFrame<'a> {
    header: EthernetHeader,
    data: &'a [u8],
}
impl<'a> EthernetFrame<'a> {
    fn size(&self) -> usize {
        (<EthernetHeader as PackedStruct>::ByteArray::len() + self.data.len())
            .max(60)
    }
    fn pack(&self, dst: &mut [u8]) {
        let mut dst = Cursor::new(dst);
        let padding = [0; 60];
        dst.write(self.header.pack().unwrap().as_bytes_slice()).unwrap();
        dst.write(self.data).unwrap();
        let pos = dst.position() as usize;
        if pos < padding.len() {
            dst.write(&padding[pos ..]).unwrap();
        }
    }
    fn unpack(src: &'a [u8]) -> Self {
        // parse header
        let header = EthernetHeader::unpack_from_slice(
            &src[.. <EthernetHeader as PackedStruct>::ByteArray::len()]
            ).unwrap();
        // extract content
        let data = &src[<EthernetHeader as PackedStruct>::ByteArray::len() ..];
        let data = &data[.. data.len().min(data.len())];
    
        Self{header, data}
    }
}

/// ethernet frame header as specified in ISO/IEC 8802-3
#[derive(PackedStruct, Clone, Debug, Eq, PartialEq)]
#[packed_struct(size_bytes="14", bit_numbering = "lsb0", endian = "msb")]
struct EthernetHeader {
    /// destination MAC address
    #[packed_field(bytes="8:13")]  dst: [u8;6],
    /// source MAC address
    #[packed_field(bytes="2:7")]  src: [u8;6],
    // vlan is said to be optional and this is not present in most ethercat frames, so will not be used here
    //#[packed_field(bytes="2:5")]  vlan: [u8;4],  
    /// ethernet protocol
    #[packed_field(bytes="0:1")]  protocol: u16,
}
// /// ethernet frame footer as specified in ISO/IEC 8802-3
// #[derive(PackedStruct, Clone, Debug, Eq, PartialEq)]
// #[packed_struct(size_bytes="4", bit_numbering = "lsb0", endian = "msb")]
// struct EthernetFooter {
//     /// checksum of the frame
//     #[packed_field(bytes="0:3")]  checksum: u32,
// }
