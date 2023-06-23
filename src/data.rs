//! Traits and impls used to read/write data to/from the wire.

use core::{fmt, marker::PhantomData};

/** Offset maximal allowed for bit shift */
pub const MAX_OFFSET: u8 = 8;

/**
	trait for data types than can be packed/unpacked to/from a PDU
*/
pub trait PduData: Sized {
    const ID: TypeId;
    type Packed: Storage;

    fn pack(&self, data: &mut [u8]) -> PackingResult<()> { self.pack_slice(data, 0, Self::packed_bitsize(), Ordering::Little) }
    fn unpack(src: &[u8]) -> PackingResult<Self> { Self::unpack_slice(src, 0, Self::packed_bitsize(), Ordering::Little)}
    
    fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<()>;
    fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<Self>;
    
    fn packed_size() -> usize {Self::Packed::LEN}
    fn packed_bitsize() -> usize {Self::Packed::LEN * 8}
}

/** Enum to identify and raise adapted error raised by this package
*/
#[derive(Copy, Clone, Debug)]
pub enum PackingError {
    BadSize(usize, &'static str),
    BadAlignment(usize, &'static str),
    BadOrdering(Ordering, &'static str),
    InvalidValue(&'static str),
}

pub type PackingResult<T> = Result<T, PackingError>;

/// this trait is an equivalent to `packed_struct::ByteArray` but since rust doesn't actually support using generic consts in const expressions, we do not have choice
pub trait Storage: AsRef<[u8]> + AsMut<[u8]>
//         + Index<Range<usize>, Output=[u8]> + IndexMut<Range<usize>, Output=[u8]>
//         + Index<usize, Output=u8> + IndexMut<usize, Output=u8>
//             + Index<SliceIndex<[u8], Output=[u8]>, Output=[u8]> // + IndexMut<usize, Output=u8>
{
    const LEN: usize;
    fn uninit() -> Self;
}

impl<const N: usize> Storage for [u8; N] {
    const LEN: usize = N;
    fn uninit() -> Self {unsafe {core::mem::uninitialized()}}
}

/** dtype identifiers associated to dtypes allowing to dynamically check the type of a [PduData] implementor

    It is only convering the common useful types and not all the possible implementors of [PduData]
*/
#[derive(Copy, Clone, Debug)]
pub enum TypeId {
    /// default value of the enum, used in case the matching [PduData] does not fit in any of these integers
    CUSTOM,
    BOOL,
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    F32, F64,
}

/** Identifier used to define the position of the MSB in the field
    @see https://en.wikipedia.org/wiki/Endianness?useskin=vector for more information
*/
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum Ordering {
    #[default] Little,
    Big,
    Middle,
}

impl std::fmt::Display for Ordering {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match &*self {
            Ordering::Big => write!(f, "Big"),
            Ordering::Little => write!(f, "Little"),
            _ => write!(f, "Mixed"),
        };
    }
}


/// macro implementing [PduData] for a given struct generated with `bilge`
/// this is an ugly unsafe code to overcome the lack of traits providing containing ints in [bilge]
macro_rules! bilge_pdudata {
    ($t: ty, $id: ident) => { impl crate::data::PduData for $t {
        // we cannot use Self.value for unknown reason
        // we cannot use $id::from(Self) for unknown reason
        // we cannot use $id::to_le_bytes because it is only implemented for byte exact integers

        const ID: crate::data::TypeId = crate::data::TypeId::CUSTOM;
        type Packed = [u8; ($id::BITS as usize + 7)/8];

        fn pack(&self, dst: &mut [u8]) -> crate::data::PackingResult<()> {
            if dst.len() < Self::Packed::LEN
                {return Err(crate::data::PackingError::BadSize(dst.len(), "bilge struct needs exact size"))}
            dst[..Self::Packed::LEN].copy_from_slice(&unsafe{ core::mem::transmute_copy::<Self, Self::Packed>(self) });
            Ok(())
        }
        fn unpack(src: &[u8]) -> crate::data::PackingResult<Self> {
            if src.len() < Self::Packed::LEN
                {return Err(crate::data::PackingError::BadSize(src.len(), "bilge struct needs exact size"))}
            let mut tmp = [0; core::mem::size_of::<Self>()];
            tmp[.. Self::Packed::LEN].copy_from_slice(&src[.. Self::Packed::LEN]);
            Ok(unsafe{ core::mem::transmute::<[u8; core::mem::size_of::<Self>()], Self>(tmp) })
        }

        fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, _bitordering: crate::data::Ordering) -> crate::data::PackingResult<()> {
            if bitoffset != 0 {
                return Err(crate::data::PackingError::BadAlignment(bitoffset as usize, "Bit offset must be at 0 on this implementation")); }
            if bitsize != 0 {
                return Err(crate::data::PackingError::BadSize(bitsize, "Bit size must be at 0 on this implementation")); }
            return self.pack(data);
        }

        fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, _bitordering: crate::data::Ordering) -> crate::data::PackingResult<Self> {
            if bitoffset != 0 {
                return Err(crate::data::PackingError::BadAlignment(bitoffset as usize, "Bit offset must be at 0 on this implementation")); }
            if bitsize != 0 {
                return Err(crate::data::PackingError::BadSize(bitsize, "Bit size must be at 0 on this implementation")); }
            return Self::unpack(src)
        }
    }};
}

pub(crate) use bilge_pdudata;

/// unsafe macro implementing [PduData] for a given struct with `repr(packed)`
macro_rules! packed_pdudata {
    ($t: ty) => { impl crate::data::PduData for $t {
        const ID: crate::data::TypeId = crate::data::TypeId::CUSTOM;
        type Packed = [u8; core::mem::size_of::<$t>()];

        fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, _bitordering: crate::data::Ordering) -> crate::data::PackingResult<()> {
            if bitoffset != 0 {
                return Err(crate::data::PackingError::BadAlignment(bitoffset as usize, "Offset not null"))}
            if bitsize > data.len() {
                    return Err(crate::data::PackingError::BadSize(bitsize, "Size too big for the slice"))}
            if data.len() < Self::Packed::LEN
                {return Err(crate::data::PackingError::BadSize(data.len(), "not enough bytes for struct"))}
            data[..Self::Packed::LEN].copy_from_slice(&unsafe{ core::mem::transmute_copy::<Self, Self::Packed>(self) });
            Ok(())
            }
        fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, _bitordering: crate::data::Ordering) -> crate::data::PackingResult<Self> {
            if bitoffset != 0 {
                return Err(crate::data::PackingError::BadAlignment(bitoffset as usize, "Offset not null"))}
            if bitsize > src.len() {
                    return Err(crate::data::PackingError::BadSize(bitsize, "Size too big for the slice"))}
            if src.len() < Self::Packed::LEN
                {return Err(crate::data::PackingError::BadSize(src.len(), "not enough bytes for struct"))}
            let src: &Self::Packed = src[.. Self::Packed::LEN].try_into().unwrap();
            Ok(unsafe{ core::mem::transmute::<Self::Packed, Self>(src.clone()) })
        }
    }};
}
pub(crate) use packed_pdudata;

impl<const N: usize> PduData for [u8; N] {
    const ID: TypeId = TypeId::CUSTOM;
    type Packed = Self;

    /** Pack data in a slice according to the following argument: <br>
    - data: Packed data - "out" value<br>
    - bitoffset: Index of the data shift. Have to be between 0 and MAX_OFFSET (=7)<br>
    - bitsize: Size of the variable type: Integer on 16 bits => 16 or Integer on 3bits => 3 <br>
    - bitordering: Endian ordering for the packed data<br>
    */
    fn pack_slice(&self, data: &mut [u8], bitoffset: u8, _bitsize: usize, bitordering: Ordering) -> PackingResult<()> {
        if bitoffset >= MAX_OFFSET {
            return Err(PackingError::BadAlignment(0, "Offset greater than 8"));
        }
        if bitordering == Ordering::Middle {
            return Err(PackingError::BadOrdering(bitordering, "Value not allowed"));
        }
        if (bitoffset == 0 && data.len() < N) || (data.len() < N + 1) {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof <$t>",
            ));
        }

        data.copy_from_slice(self);

        #[cfg(target_endian = "big")]
        if bitordering == Ordering::Little {
            data.reverse()
        };

        #[cfg(target_endian = "little")]
        if bitordering == Ordering::Big {
            data.reverse()
        };

        for i in N..1 {
            data.swap(N - i - 1, N - i - 2)
        }

        let mut i: usize = N - 1;
        while i > 0 {
            if i < bitoffset as usize {
                data[i] = 0;
            } else {
                data[i] = data[i - bitoffset as usize];
            }
            i -= 1;
        }

        return Ok(());
    }

    fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<Self> {
        //Test input parameters
        let array_len_bit = src.len() * 8;
        let field_len_bit = N * 8;
        if bitoffset >= MAX_OFFSET {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "Alignement superior than one byte",
            ));
        }
        if bitsize + bitoffset as usize > array_len_bit || bitsize > field_len_bit {
            return Err(PackingError::BadSize(
                bitsize,
                "Size greater than input slice size",
            ));
        }
        if (bitoffset == 0 && src.len() < N) || (src.len() < N + 1) {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof <$t>",
            ));
        }

        let mut data: [u8; N] = [0; N];

        for i in 0..N {
            if i > 0 {
                data[i - 1] |= src[i] >> (8 - bitoffset) as u8;
            }
            data[i] = src[i] << bitoffset;
        }

        if bitordering == Ordering::Little {
            data.reverse()
        };

        return Ok(data.try_into().unwrap());
    }
}

impl PduData for f32 {
    const ID: TypeId = TypeId::F32;
    type Packed = [u8;4];

    fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<()> {
        if bitoffset != 0 {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "For this type, offset different than 0 is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<f32>() {
            return Err(PackingError::BadSize(
                bitoffset as usize,
                "For this type,the size cannot be different than 32",
            ));
        }
        if data.len() < core::mem::size_of::<f32>() {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof 4",
            ));
        }

        let cpy: [u8; core::mem::size_of::<f32>()] = match bitordering {
            Ordering::Little => self.to_le_bytes(),
            Ordering::Big => self.to_be_bytes(),
            _ => {
                return Err(PackingError::BadOrdering(
                    bitordering,
                    "Mixed endian is invalid in this case",
                ))
            }
        };

        for i in 0..core::mem::size_of::<f32>() {
            data[i] = cpy[i];
        }
        return Ok(());
    }

    fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<Self> {
        if bitoffset != 0 {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "For this type, offset different than 0 is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<f32>() {
            return Err(PackingError::BadSize(
                bitoffset as usize,
                "For this type,the size cannot be different than 32",
            ));
        }
        if src.len() < core::mem::size_of::<f32>() {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof 4",
            ));
        }

        return Ok(match bitordering {
            Ordering::Little => f32::from_le_bytes(src.try_into().unwrap()),
            Ordering::Big => f32::from_be_bytes(src.try_into().unwrap()),
            _ => { return Err(PackingError::BadOrdering( bitordering, "Mixed endian is invalid in this case" ))}
        });
    }
}

impl PduData for f64 {
    const ID: TypeId = TypeId::F64;
    type Packed = [u8;8];

    fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<()> {
        if bitoffset != 0 {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "For this type, offset different than 0 is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<f64>() {
            return Err(PackingError::BadSize(
                bitoffset as usize,
                "For this type,the size cannot be different than 32",
            ));
        }
        if data.len() < core::mem::size_of::<f64>() {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof 8",
            ));
        }

        let cpy: [u8; core::mem::size_of::<f64>()] = match bitordering {
            Ordering::Little => self.to_le_bytes(),
            Ordering::Big => self.to_be_bytes(),
            _ => {
                return Err(PackingError::BadOrdering(
                    bitordering,
                    "Mixed endian is invalid in this case",
                ))
            }
        };

        for i in 0..core::mem::size_of::<f64>() {
            data[i] = cpy[i];
        }
        return Ok(());
    }

    fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize,  bitordering: Ordering) -> PackingResult<Self> {
        if bitoffset != 0 {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "For this type, offset different than 0 is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<f64>() {
            return Err(PackingError::BadSize(
                bitoffset as usize,
                "For this type,the size cannot be different than 64",
            ));
        }
        if src.len() < core::mem::size_of::<f64>() {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof 8",
            ));
        }

        return Ok(match bitordering {
            Ordering::Little => f64::from_le_bytes(src.try_into().unwrap()),
            Ordering::Big => f64::from_be_bytes(src.try_into().unwrap()),
            _ => {
                return Err(PackingError::BadOrdering(
                    bitordering,
                    "Mixed endian is invalid in this case",
                ))
            }
        });
    }
}

impl PduData for bool {
    const ID: TypeId = TypeId::BOOL;
    type Packed = [u8;1];

    fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, _bitordering: Ordering) -> PackingResult<()> {
        if bitoffset >= MAX_OFFSET {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "Offset greater tan 8 excluded is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<bool>() {
            return Err(PackingError::BadSize(bitoffset as usize, "For this type,the size cannot be different than 1")); }
        if bitoffset == 0 && data.len() != 1 {
            return Err(PackingError::InvalidValue("Data slice too short, must equal or greater than sizeof 1")); }

        data[0] = u8::from(match self { true => 1,  _ => 0, }) >> bitoffset;

        return Ok(());
    }

    fn unpack_slice(src: &[u8],  bitoffset: u8, bitsize: usize,  _bitordering: Ordering) -> PackingResult<Self> {
        if bitoffset != 0 {
            return Err(PackingError::BadAlignment(bitoffset as usize, "For this type, offset different than 0 is not supported" )); }
        if bitsize != core::mem::size_of::<bool>() {
            return Err(PackingError::BadSize(bitoffset as usize, "For this type,the size cannot be different than 1")); }
        if bitoffset == 0 && src.len() != 1 {
            return Err(PackingError::InvalidValue("Data slice too short, must equal or greater than sizeof 1" )); }

        return Ok(src[0] << bitoffset == 1);
    }
}

macro_rules! num_pdudata {
    ($t: ty, $id: ident) => {
        impl crate::data::PduData for $t {
            const ID: TypeId = crate::data::TypeId::$id;
            type Packed = [u8; core::mem::size_of::<Self>()];

            /** Pack data in a slice according to the following argument: <br>
             * Data are packed following these step: ordering > offset > trunc
             - data: Packed data - "out" value<br>
             - bitoffset: Index of the data shift. Have to be between 0 and MAX_OFFSET (=7)<br>
             - bitsize: Size of the variable type: Integer on 16 bits => 16 or Integer on 3bits => 3 <br>
             - bitordering: Endian ordering for the packed data<br>

            ```mermaid
            graph TD;
                Input-->A[Get unsed bits];Z
                A-->B[Clear unsed bits];
                A-->F[Value to byte array];
                B-->C[Bit ordering];
                C-->D[Value to array];
                D-->E[Shift offset];
                E-->F[array to value]
                F-->H[Set unsed bits];
                H-->I[Value to array];
                I-->Output
            ```
            Caveat overlap the end of slice
            */
            fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<()>
            {
                //Test input parameters
                let array_len_bit : usize = data.len() * 8;
                let field_len_bit : usize = core::mem::size_of::<$t>() * 8;
                let excluded_bits : u8 = 8 - ((usize::from(bitoffset) + bitsize) % 8) as u8;

                if bitsize == 0 {
                    return Err(PackingError::InvalidValue("Bit size couldn't be negative")); }
                if bitoffset >= MAX_OFFSET {
                    return Err(PackingError::BadAlignment(bitoffset as usize, "Offset greater than 8")); }
                if bitsize + bitoffset as usize > array_len_bit {
                    return Err(PackingError::BadSize(bitsize, "Data too large for the given slice")); }
                
                //Get unsed data
                let prefix_mask : u8 = match bitoffset > 0 {
                    true => ( data[0] >> excluded_bits ) << excluded_bits,
                    false => 0
                };
                
                //Ordering byte
                let ordering_bits = match bitordering {
                    Ordering::Big => self.to_be_bytes(),
                    Ordering::Little => self.to_le_bytes(),
                    _ => return Err(PackingError::BadOrdering(bitordering,"Mixed endian is invalid in this case"))
                };
                
                //Get suffix mask
                let suffix_mask : u8 = match bitoffset > 0 {
                    true => (ordering_bits[ordering_bits.len() -1] << excluded_bits),
                    _ => 0,
                };
                
                //Clear non used data
                // Exemple (on 1 byte, with offset = 3 and bitsize = 4 | d d d d * * * * | -> | d d d d 0 0 0 0 | -> | 0 0 0  d d d d 0 |
                // Exemple (on 1 byte, with offset = 3 and bitsize = 7 | d d d d d d d * | -> | d d d d d d d 0 | -> | 0 0 0  d d d d d |
                let mut core = <$t>::from_be_bytes(ordering_bits);
                core >>= (field_len_bit - bitsize) as $t;
                core <<= (field_len_bit - bitsize) as $t;
                core >>= bitoffset;
                let array = core.to_be_bytes();
                
                //Format final data
                for i in 0 .. (bitsize + 7) / 8
                {
                    if i == 0 { data[i] = array[i] | prefix_mask; }
                    else { data[i] = array[i] ; }
                }
                if (bitsize + usize::from(bitoffset)) % 8 > 0 {
                    data[(bitsize + usize::from(bitoffset))/ 8] |= suffix_mask; 
                }
                
                return Ok(());
            }

            fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, bitordering: Ordering) -> PackingResult<Self> {
                //Test input parameters
                let array_len_bit = src.len() * 8;
                let field_len_bit = core::mem::size_of::<$t>() * 8;
                let excluded_bits : u8 = 8 - ((usize::from(bitoffset) + bitsize) % 8) as u8;
                
                if bitoffset >= MAX_OFFSET {
                    return Err(PackingError::BadAlignment(bitoffset as usize, "Alignement superior than one byte")); }
                if bitsize + bitoffset as usize > array_len_bit || bitsize > field_len_bit {
                    return Err(PackingError::BadSize(bitsize, "Size greater than input slice size" )); }

                //Extract data and suffix
                let mut buf = [0; core::mem::size_of::<$t>()];
                let bufsize = src.len().min(buf.len());
                buf[.. bufsize].copy_from_slice(&src[.. bufsize]);
                let mut data : $t = <$t>::from_be_bytes(buf);
                data <<= bitoffset;
                let mut data = data.to_be_bytes();
                let suffix : u8 = match src.len() > core::mem::size_of::<$t>() {
                    true => src[core::mem::size_of::<$t>()],
                    _ => 0,
                };

                //Concatenate suffix
                if bitsize + bitoffset as usize > field_len_bit {
                    let shift = suffix >> excluded_bits ;
                    data[bitsize / 8] |= shift;
                }

                //Ordering
                return Ok(match bitordering {
                    Ordering::Big => Self::from_be_bytes(data),
                    Ordering::Little => Self::from_le_bytes(data),
                    _ => return Err(PackingError::BadOrdering(bitordering, "Mixed endian is invalid in this case"))
                });
            }
        }
    };
}

num_pdudata!(u8, U8);
num_pdudata!(u16, U16);
num_pdudata!(u32, U32);
num_pdudata!(u64, U64);
num_pdudata!(i8, I8);
num_pdudata!(i16, I16);
num_pdudata!(i32, I32);
num_pdudata!(i64, I64);

/**
    locate some data in a datagram by its byte position and length, which must be extracted to type `T` to be processed in rust

    It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Field<T: PduData> {
    extracted: PhantomData<T>,
    /// start byte index of the object
    pub offset: usize,
    /// byte length of the object
    pub len: usize,
    /// define byte the alignement used by the field. Can be set only on "Ctor."
    pub endian: Ordering,
}

impl<T: PduData> Field<T> {
    /// build a Field from its content
    pub fn new(byte: usize, len: usize) -> Self {
        Self {extracted: PhantomData, offset: byte, len, endian: Ordering::Little}
    }

    /// build a Field from its byte offset, infering its length from the data nominal size
	pub const fn simple(byte: usize) -> Self {
        Self {extracted: PhantomData, offset: byte, len: T::Packed::LEN, endian : Ordering::Little }
	}

    /// Build a field with default value:
    /// - 0 byte offset
    /// - 0 bit offset
    /// - Lenght = field type size
    /// - Little endian by ordering
    pub fn default() -> Self {
        Self {extracted: PhantomData, offset: 0, len: core::mem::size_of::<T>(), endian: Ordering::Little }
    }

    /// extract the value pointed by the field in the given byte array
    pub fn get(&self, data: &[u8]) -> PackingResult<T> {
        return T::unpack_slice(&data[self.offset..], 0, self.get_bits_len(), self.endian);
    }

    /// dump the given value to the place pointed by the field in the byte array
    pub fn set(&self, data: &mut [u8], value: T) -> PackingResult<()> {
        let mut sub_data = &mut data[self.offset..];
        return value.pack_slice(&mut sub_data, 0, self.get_bits_len(), self.endian);
    }

    /// Return field size in **byte**
    /// Caveat: This value can be different than size of the type
    pub fn len(&self) -> usize {
        return self.len;
    }

    /// Return the field size in **bit**
    pub fn get_bits_len(&self) -> usize {
        return self.len * 8;
    }

}

impl<T: PduData> fmt::Debug for Field<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.offset > 0 {
            write!(f, "Field{{ Size: {}, Endian: {} }}", self.len, self.endian) }
        else {
            write!(f, "Field{{ Offset: {}, Size: {}, Endian: {} }}", self.offset, self.len, self.endian) }

    }
}

impl<T: PduData> std::convert::TryFrom<BitField<T>> for Field<T> {
    type Error = PackingError;

    /// Comsusme a Bitfield struct and perform a transmutation into Field
    /// Caveat: If alignement is different thant 0 or size is not a multiple of 8 an error is throw.
    fn try_from(src : BitField<T>) -> Result<Self, Self::Error> {
        if src.offset != 0 {
            return Err(PackingError::BadAlignment(src.offset, "Bit alignement different from 0")); }
        return match src.len % 8 {
            0 => Ok(Self { extracted : src.extracted, offset : 0, len : src.len() / 8, endian : src.endian }),
            _ => Err(PackingError::BadSize(src.len(), "Must be a multiple of 8"))
        };
    }
}

/**
    locate some data in a datagram by its bit position and length, which must be extracted to type `T` to be processed in rust

    It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Clone)]
pub struct BitField<T: PduData> {
    extracted: PhantomData<T>,
    /// start bit index of the object
    pub offset: usize,
    /// bit length of the object
    pub len: usize,
    /// define bit the ordering used by the field. Can be set only on "Ctor."
    pub endian: Ordering,
}

impl<T: PduData> BitField<T> {
    /// build a Field from its content
    pub fn new_full(offset: usize, len: usize, endian: Ordering) -> Self {
        Self { extracted: PhantomData,  offset, len, endian }
    }

    pub const fn new(offset: usize, len: usize) -> Self {
        Self { extracted: PhantomData,  offset, len, endian : crate::data::Ordering::Little }
    }

    pub fn default() -> Self {
        Self { extracted: PhantomData, offset: 0, len: 0, endian: Ordering::Little}
    }

    /// extract the value pointed by the field in the given byte array
    pub fn get(&self, data: &[u8]) -> PackingResult<T> {
        T::unpack_slice(&data[self.offset / 8 ..], (self.offset % 8) as u8, self.len, self.endian)
    }
    /// dump the given value to the place pointed by the field in the byte array
    pub fn set(&self, data: &mut [u8], value: T) -> PackingResult<()> {
        value.pack_slice(&mut data[self.offset / 8 ..], (self.offset % 8) as u8, self.len, self.endian)
    }

    /// Return field size in **bit**
    pub fn len(&self) -> usize {
        return self.len;
    }

}

impl<T: PduData> fmt::Debug for BitField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BitField{{ Offset: {}, Size: {},  Endian: {} }}", self.offset, self.len, self.endian)
    }
}

impl<T:PduData> std::convert::TryFrom<Field<T>> for BitField<T> {
    type Error = PackingError;

    /// Consume a Field struct and perfom a transmutation into Bitfield
    fn try_from(src : Field<T>) -> Result<Self, Self::Error> {
        if src.offset != 0 {
            return Err(PackingError::BadAlignment(src.offset, "Bit alignement different from 0")); }
        return Ok(Self { extracted: src.extracted, offset: 0, len: src.len() * 8, endian: src.endian });
    }
}

/** helper to read/write sequencial data from/to a byte slice

    It is close to what [std::io::Cursor] is doing, but this struct allows reading forward without consuming the stream, and returns slices without copying the data. It is also meant to work with [PduData]

    Depending on the mutability of the slice this struct is built on, different capabilities are provided.
*/
pub struct Cursor<T> {
    position: usize,
    data: T,
}

impl<T> Cursor<T> {
    /// create a new cursor starting at position zero in the given slice
    pub fn new(data: T) -> Self   {Self{position: 0, data}}
    /** current position in the read/write slice

        bytes before this position are considered read or written, and bytes after are coming for use in next read/write calls
    */
    pub fn position(&self) -> usize   {self.position}
}

impl<'a> Cursor<&'a [u8]> {
    /// read the next coming bytes with a [PduData] value, and increment the position
    pub fn unpack<T: PduData>(&mut self) -> PackingResult<T> {
        let start = self.position.clone();
        self.position += T::Packed::LEN;
        T::unpack(&self.data[start .. self.position])
    }
    /// read the next coming `size` bytes and increment the position
    pub fn read(&mut self, size: usize) -> PackingResult<&'_ [u8]> {
        let start = self.position.clone();
        self.position += size;
        Ok(&self.data[start .. self.position])
    }
    /// return all the remaining bytes after current position, but does not advance the cursor
    pub fn remain(&self) -> &'a [u8] {
        &self.data[self.position ..]
    }
    /// consume self and return a slice until current position
    pub fn finish(self) -> &'a [u8] {
        &self.data[.. self.position]
    }
}

impl<'a> Cursor<&'a mut [u8]> {
    /// read the next coming bytes with a [PduData] value, and increment the position
    pub fn unpack<T: PduData>(&mut self) -> PackingResult<T> {
        let start = self.position.clone();
        self.position += T::Packed::LEN;
        T::unpack(&self.data[start .. self.position])
    }
    /// read the next coming `size` bytes and increment the position
    pub fn read(&'a mut self, size: usize) -> PackingResult<&'_ [u8]> {
        let start = self.position.clone();
        self.position += size;
        Ok(&self.data[start .. self.position])
    }
    /// write the next coming bytes with a [PduData] value, and increment the position
    pub fn pack<T: PduData>(&mut self, value: &T) -> PackingResult<()> {
        let start = self.position.clone();
        self.position += T::Packed::LEN;
        value.pack(&mut self.data[start .. self.position])
    }
    /// write the next coming bytes with the given slice, and increment the position
    pub fn write(&mut self, value: &[u8]) -> PackingResult<()> {
        let start = self.position.clone();
        self.position += value.len();
        self.data[start .. self.position].copy_from_slice(value);
        Ok(())
    }
    /// return all the remaining bytes after current position, but does not advance the cursor
    pub fn remain(&mut self) -> &'_ mut [u8] {
        &mut self.data[self.position ..]
    }
    /// consume self and return a slice until current position
    pub fn finish(self) -> &'a mut [u8] {
        &mut self.data[.. self.position]
    }
}




#[test]
fn test_aligned() {
    use core::fmt::Debug;
    
    fn test<T: PduData + PartialEq + Debug>(data: T, reference: &[u8]) {
        let mut buf = T::Packed::uninit();
        buf.as_mut().fill(0);
        data.pack(buf.as_mut()).unwrap();
        
        assert_eq!(buf.as_ref(), reference);
        assert_eq!(T::unpack(buf.as_ref()).unwrap(), data);
    }
    
    test(0u8, &[0x00]);
    test(0u32, &[0x00; 4]);
    test(1u8, &[0x01]);
    test(1u16, &[0x01, 0x00]);
    test(0x1234u16, &[0x34, 0x12]);
    test(0x123456u32, &[0x56, 0x34, 0x12, 0x00]);
}

#[test]
fn test_field() {
    use core::fmt::Debug;
    
    fn test
        <T: PduData + PartialEq + Debug + Clone, const N: usize>
        (data: T, field: Field<T>, ref_value: T, ref_packed: &[u8; N]) 
    {
        let mut buf = [0; N];
        field.set(&mut buf, data.clone()).unwrap();
        
        assert_eq!(buf.as_slice(), ref_packed.as_slice());
        assert_eq!(field.get(&buf).unwrap(), ref_value);
    }
    
    test(0u8, Field::new(1,1), 0, &[0, 0]);
    test(0u16, Field::new(1,2), 0, &[0, 0, 0]);
    test(1u8, Field::new(1,1), 1, &[0, 0x01]);
    test(1u16, Field::new(1,2), 1, &[0, 0x01, 0]);
    test(0x1234u16, Field::new(1,2), 0x1234, &[0, 0x34, 0x12, 0]);
    test(0x123456u32, Field::new(1,2), 0x3456, &[0, 0x56, 0x34, 0]);
}

#[test]
fn test_bitfield() {
    use core::fmt::Debug;
    
    fn test
        <T: PduData + PartialEq + Debug + Clone, const N: usize>
        (data: T, field: BitField<T>, ref_value: T, ref_packed: &[u8; N]) 
    {
        let mut buf = [0; N];
        field.set(&mut buf, data.clone()).unwrap();
        
        assert_eq!(buf.as_slice(), ref_packed.as_slice());
        assert_eq!(field.get(&buf).unwrap(), ref_value);
    }
    
    test(0u8, BitField::new(3,7), 0, &[0, 0]);
    test(0u16, BitField::new(3,15), 0, &[0, 0, 0]);
    test(0b10u8, BitField::new(3,7), 0b10, &[0, 0b10_000000]);
    test(0b10u16, BitField::new(3,15), 0b10, &[0, 0b01000000, 0]);
    
    
    
    use bilge::prelude::*;
    
    #[bitsize(24)]
    struct tutu {
        padding: u3,
        v: u14,
        padding: u7,
    }
    println!("bilge {:?}", tutu::new(u14::new(0b00_0110_0101_1111_u16)).value.to_le_bytes());
    
    #[bitsize(32)]
    struct tata {
        padding: u6,
        v: u4,
        padding: u22,
    }
    println!("bilge {:?}", tata::new(u4::new(0b1101)).value);
    
    
//      0b1_1111_000  0b011_0010
//         0b_0011_0010_1111_1000
    test(0b1100_0110_0101_1111_u16, 
        BitField::new(3,14), 
        0b1100_0110_0101_1111_u16, 
        &[0b0001_1000, 0b1100_1000, 0],
        );
//     test(0b00001100_01100101_11111001_10100011_u32, 
//         BitField::new(3,14), 
//         0b00001100_01100101_11111001_10100011_u32, 
//         &[0b00000011, 0b00011000, 0b00000000, 0b0000000, 0],
//         );
        
}

#[test]
fn test_shit_bits(){
    println!("{:016b}-{:016b}", 0xFFu16, 0xFFu16 << 3); 
}

#[test]
fn test_old() {
    // Pdu field
    let a : Field::<u16> = Field::default();
    let mut c: [u8; 6] = [0; 6];
    a.set(&mut c[0..], 5).expect("Error on setting data");

    let b : Field::<u32> = Field::new(2, 2);
    b.set(&mut c[0..], 5).expect("Error on setting data");
    println!("{:#?}",c);
    println!("{:#?}",a.get(&c));

    c.fill(0);
    let d : BitField<u16> = BitField::new(2, 11);
    d.set(&mut c[2..], 5);

    println!("i: c");
    println!("{:#?}",d);
    for i in 0..6{ println!("{}: {:b} ",i, c[i]) }

    println!("{:#?}", Field::try_from(d));

    println!("{:#?}", BitField::<u32>::default().try_into() as Result<Field<u32>,PackingError>);

    let gg : BitField<u16> = a.try_into().unwrap();
    println!("{:#?}", gg );

    //Bilge
    // let a1: MyStructInt = MyStructInt::new(5);
    // let b2: u32 = a1.len() as u32;
    // println!("{:#02x}",a1.value);
    // println!("{:#?} ",b2);

    let a: Field<u8> = Field::simple(0x6);
    let b: BitField<u8> = BitField::new(0x4, 8);
    let mut c : [u8; 0x8] = [0;0x8];
    let mut d : [u8; 0x8] = [0;0x8];
    a.set(&mut c[0..], 27);
    b.set(&mut d[0..], 27);
    println!("Print c:");
    for i in 0..c.len(){ println!("{}: {:b} ",i, c[i]) }
    println!("Print d:");
    for i in 0..d.len(){ println!("{}: {:b} ",i, d[i]) }

    let e: Field<u8> = Field::simple(0x6);
    let f: BitField<u8> = BitField::new(0x4, 8);

    let g = e.get(&c[0..]).unwrap();
    let h = f.get(&d[0..]).unwrap();
    println!("{}",g);
    println!("{}",h);
}

