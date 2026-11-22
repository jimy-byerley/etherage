//! Vendor-specific mailbox payload framing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub vendor_id: u32,
    pub vendor_type: u16,
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn decode(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 6 {
            return None;
        }
        Some(Self {
            vendor_id: u32::from_le_bytes(bytes[..4].try_into().ok()?),
            vendor_type: u16::from_le_bytes(bytes[4..6].try_into().ok()?),
            payload: &bytes[6..],
        })
    }

    pub fn encode(self, output: &mut [u8]) -> Option<usize> {
        let end = 6usize.checked_add(self.payload.len())?;
        let header = output.get_mut(..6)?;
        header[..4].copy_from_slice(&self.vendor_id.to_le_bytes());
        header[4..].copy_from_slice(&self.vendor_type.to_le_bytes());
        output.get_mut(6..end)?.copy_from_slice(self.payload);
        Some(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_vendor_payload() {
        let packet = Packet { vendor_id: 0x1337, vendor_type: 1, payload: &[1, 2] };
        let mut bytes = [0; 8];
        assert_eq!(packet.encode(&mut bytes), Some(8));
        assert_eq!(Packet::decode(&bytes), Some(packet));
        assert_eq!(Packet::decode(&bytes[..5]), None);
    }

    #[test]
    fn vendor_echo_vector_matches_standard_header() {
        let packet = Packet { vendor_id: 0x1337, vendor_type: 1, payload: &[0xaa] };
        let mut bytes = [0; 7];
        assert_eq!(packet.encode(&mut bytes), Some(7));
        assert_eq!(bytes, [0x37, 0x13, 0, 0, 1, 0, 0xaa]);
    }
}
