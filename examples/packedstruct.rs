use packed_struct::prelude::PackedStruct;

#[derive(Default, Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PduCommand {
    /// no operation
    #[default]
    NOP = 0x0,
    
    /// broadcast read
    BRD = 0x07,
    /// broadcast write
    BWR = 0x08,
    /// broadcast read & write
    BRW = 0x09,
    
    /// auto-incremented read
    APRD = 0x01,
    /// auto-incremented write
    APWR = 0x02,
    /// auto-incremented read & write
    APRW = 0x03,
    
    FPRD = 0x04,
    FPWR = 0x05,
    FPRW = 0x06,
    
    LRD = 0x0A,
    LWR = 0x0B,
    LRW = 0x0C,
    
    ARMW = 0x0D,
    FRMW = 0x0E,
}
#[derive(PackedStruct, Clone, Debug, Default)]
#[packed_struct(size_bytes="9", bit_numbering = "lsb0", endian = "lsb")]
struct PduHeader {
    #[packed_field(bytes="0")]  command: PduCommand,
    #[packed_field(bytes="1")]  token: u8,
    #[packed_field(bytes="2:3")]    destination: u16,
    #[packed_field(bytes="4:5")]    address: u16,
    #[packed_field(bits="16:26")]   len: u16,
    #[packed_field(bits="30")]  circulating: bool,
    #[packed_field(bits="31")]  next: bool,
    #[packed_field(bytes="8")]  irq: u8,
}

fn main() {
	PduHeader {
		command: PduCommand::BRD,
		destination: 0x1234,
		.. Default::default()
		};
}
