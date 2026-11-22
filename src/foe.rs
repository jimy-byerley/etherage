//! File Access over EtherCAT packet encoding.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    Read = 1,
    Write = 2,
    Data = 3,
    Ack = 4,
    Error = 5,
    Busy = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub opcode: Opcode,
    pub value: u32,
    pub data: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    UnknownOpcode,
}

/// Transfer failures, preserving a FoE error code reported by the slave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoeError {
    /// Mailbox transport or framing failure.
    Mailbox(crate::mailbox::MailboxError),
    /// FoE error code from an `ERROR` response (usually in the 0x8000 range).
    Remote(u32),
}

impl<'a> Packet<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        if bytes.len() < 6 {
            return Err(DecodeError::TooShort);
        }
        let opcode = match u16::from_le_bytes([bytes[0], bytes[1]]) {
            1 => Opcode::Read,
            2 => Opcode::Write,
            3 => Opcode::Data,
            4 => Opcode::Ack,
            5 => Opcode::Error,
            6 => Opcode::Busy,
            _ => return Err(DecodeError::UnknownOpcode),
        };
        Ok(Self {
            opcode,
            value: u32::from_le_bytes(bytes[2..6].try_into().unwrap()),
            data: &bytes[6..],
        })
    }

    pub fn encode(self, output: &mut [u8]) -> Option<usize> {
        let end = 6usize.checked_add(self.data.len())?;
        let header = output.get_mut(..6)?;
        header[..2].copy_from_slice(&(self.opcode as u16).to_le_bytes());
        header[2..].copy_from_slice(&self.value.to_le_bytes());
        output.get_mut(6..end)?.copy_from_slice(self.data);
        Some(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_rejects_bad_packets() {
        let packet = Packet { opcode: Opcode::Data, value: 7, data: &[0xaa] };
        let mut bytes = [0; 7];
        assert_eq!(packet.encode(&mut bytes), Some(7));
        assert_eq!(Packet::decode(&bytes), Ok(packet));
        assert_eq!(Packet::decode(&bytes[..5]), Err(DecodeError::TooShort));
        bytes[..2].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(Packet::decode(&bytes), Err(DecodeError::UnknownOpcode));
    }

    #[test]
    fn data_and_ack_match_soem_wire_bytes() {
        let mut data = [0; 8];
        Packet { opcode: Opcode::Data, value: 1, data: &[0xaa, 0xbb] }.encode(&mut data);
        assert_eq!(data, [3, 0, 1, 0, 0, 0, 0xaa, 0xbb]);
        let mut ack = [0; 6];
        Packet { opcode: Opcode::Ack, value: 1, data: &[] }.encode(&mut ack);
        assert_eq!(ack, [4, 0, 1, 0, 0, 0]);
    }

    #[test]
    fn rrq_and_wrq_match_the_six_byte_foe_header() {
        for (opcode, expected) in [
            (Opcode::Read, [1, 0, 0, 0, 0, 0, b'd', b'e', b'm', b'o', b'.', b'b', b'i', b'n']),
            (Opcode::Write, [2, 0, 0, 0, 0, 0, b'd', b'e', b'm', b'o', b'.', b'b', b'i', b'n']),
        ] {
            let packet = Packet { opcode, value: 0, data: b"demo.bin" };
            let mut bytes = [0; 14];
            assert_eq!(packet.encode(&mut bytes), Some(expected.len()));
            assert_eq!(bytes, expected);
        }
    }

    #[test]
    fn foe_error_code_8000_is_preserved() {
        let packet = Packet { opcode: Opcode::Error, value: 0x8000, data: &[] };
        let mut bytes = [0; 6];
        packet.encode(&mut bytes);
        assert_eq!(Packet::decode(&bytes).unwrap().value, 0x8000);
    }
}
