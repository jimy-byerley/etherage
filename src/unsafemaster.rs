use crate::socket::*;

/**
    low level ethercat functions, with no compile-time checking of the communication states
    this struct has no notion of slave, it is just executing basic commands
    
    genericity allows to use a UDP socket or raw ethernet socket
*/
pub struct RawMaster<S: Socket> {
    /// (byte) if non-null, this is the amount of PDU data to be accumulated in the send buffer before sending
	merge_packets_size: usize,
	/// (µs) acceptable delay time before sending buffered PDUs
	merge_packets_time: usize,
	
	// socket implementation
	socket: S,
	
	tokencount: u8,
	bsend: Vec<u16>,
	breceive: Vec<u16>,
}
impl RawMaster {
	pub fn new() -> Self
	
	pub fn nop(&self)
	pub fn bwr(&self)
	pub fn brd(&self)
	pub fn aprd<T: PduData>(&self, address: u16) -> T {
        let token = self.send(APRD, 0, address, data);
        self.receive(token).await.data.unpack()
	}
	pub fn apwr(&self)
	pub fn aprw(&self)
	pub fn armw(&self)
	pub fn fprd<T: PduData>(&self, slave: u16, address: u16) -> T {
        let token = self.send(FPRD, slave, address, data);
        self.receive(token).await.data.unpack()
	}
	pub fn fpwr(&self)
	pub fn fprw(&self)
	pub fn frmw(&self)
	pub fn lrd(&self)
	pub fn lwr(&self)
	pub fn lrw(&self)
	
	/// maps to a *wr command
	pub fn write(&self, address: Address, data: PduData)
	/// maps to a *rd command
	pub fn read(&self, address: Address, length: usize)
	/// maps to a *rw command
	pub fn exchange(&self, address: Address, send: Pdu, receive: usize)
	
	/// send a PDU on the ethercat bus
	/// the PDU is buffered if possible
	fn send<T>(&mut self, command: u16, destination: u16, address: u16, data: T) -> u8 {
        let padding = [0u8; 30];
        
        let data = data.pack().as_bytes_slice();
        self.bsend_last = self.bsend.len();
        self.bsend.append(PduHeader{
            command,
            token: self.tokencount,
            destination,
            address,
            len: data.len(),
            circulating: false,
            next: true,
            irq: 0,
            }.pack().as_bytes_slice())
        self.bsend.append(data);
        if data.len() < padding.len() {
            self.bsend.append(padding[data.len()..padding.len()]);
        }
        self.bsend.append(PduFooter {
            wck: 0,
            }.pack().as_bytes_slice());
        self.tokenamount = self.tokenamount.overflowing_add(1);
        
        self.autoflush();
	}
	
	fn autoflush(&mut self) {
        if Instant::now().elapsed_since(self.tsend).as_microseconds() >= self.merge_packets_time
        || self.bsend.len() >= self.merge_packets_size {
            self.flush();
        }
	}
	
	fn flush(&mut self) {
        // change last field PduHeader.next
        let offset = ((&header) as *const _ as usize) - ((&header.next) as *const _ as usize);
        self.bsend[self.bsend_last+offset] |= 1;
        // send
        self.socket.
        // reset
        self.bsend.clear();
        self.bsend_last = 0;
	}
}

/// dynamically specifies a destination address on the ethercat loop
pub enum Address {
	/// every slave will receive and execute
	Broadcast,
	/// address will be determined by the topology (index of the slave in the ethernet loop)
	AutoIncremented,
	/// address has been set by the master previously
	Configured(u32),
	/// the logical memory is the destination, all slaves are concerned
	Logical,
}

