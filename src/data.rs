//! Traits and impls used to read/write data to/from the wire.

use core::{fmt, marker::PhantomData};

/** Offset maximal allowed for bit shift */
pub const MAX_OFFSET: u8 = 8;

/** dtype identifiers associated to dtypes allowing to dynamically check the type of a [PduData] implementor

    It is only convering the common useful types and not all the possible implementors of [PduData]
*/
#[derive(Copy, Clone, Debug)]
pub enum TypeId {
    /// default value of the enum, used in case the matching [PduData] does not fit in any of these integers
    CUSTOM,
    BOOL,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

/** Identifier used to define the position of the MSB in the field
    @see https://en.wikipedia.org/wiki/Endianness?useskin=vector for more information
*/
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Endiannes {
    Little,
    Big,
    Middle,
}

/** Implement default value for bits ordering as LE - little endian */
impl Default for Endiannes {
    fn default() -> Self {
        Self::Little
    }
}

impl std::fmt::Display for Endiannes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match &*self {
            Endiannes::Big => write!(f, "Big"),
            Endiannes::Little => write!(f, "Little"),
            _ => write!(f, "Mixed"),
        };
    }
}

/** Enum to identify and raise adapted error raised by this package
*/
#[derive(Copy, Clone, Debug)]
pub enum PackingError {
    BadSize(usize, &'static str),
    BadAlignment(usize, &'static str),
    BadOrdering(Endiannes, &'static str),
    InvalidValue(&'static str),
}

pub type PackingResult<T> = Result<T, PackingError>;

// impl Error for PackingError { }

/**
    trait for data types than can be packed/unpacked to/from a PDU

    This trait is very close to [packed_struct::PackedStruct], but is distinct because struct implementing `PackedStruct` might not be meant for exchange in an ethercat PDU. However it can be easily declared as such when `Packed` is already implemented, by implementing [PduStruct] as well.

    The good practice for using `Packed` in combination with `PduData` is following this example:

        #[derive(PackedStruct)]
        struct MyStruct { ... }
        impl PduStruct for MyStruct {}

        // now PduData is now implemented using `Packed`

    It is also fine to implement [PduData] the regular way
*/
pub trait PduData: Sized {
    const ID: TypeId;
    const PACKED_SIZE: usize;

    fn pack(&self, data: &mut [u8]) -> PackingResult<()> {
        self.pack_slice(data, 0, 0, Endiannes::Little)
    }
    fn unpack(src: &[u8]) -> PackingResult<Self> {
        Self::unpack_slice(src, 0, 0, Endiannes::Little)
    }
    fn pack_slice(
        &self,
        data: &mut [u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<()>;
    fn unpack_slice(
        src: &[u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<Self>;
}

impl<const N: usize> PduData for [u8; N] {
    const ID: TypeId = TypeId::CUSTOM;
    const PACKED_SIZE: usize = N;

    /** Pack data in a slice according to the following argument: <br>
    - data: Packed data - "out" value<br>
    - bitoffset: Index of the data shift. Have to be between 0 and MAX_OFFSET (=7)<br>
    - bitsize: Size of the variable type: Integer on 16 bits => 16 or Integer on 3bits => 3 <br>
    - bitordering: Endian ordering for the packed data<br>
    */
    fn pack_slice(
        &self,
        data: &mut [u8],
        bitoffset: u8,
        _bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<()> {
        if bitoffset >= MAX_OFFSET {
            return Err(PackingError::BadAlignment(0, "Offset greater than 8"));
        }
        if bitordering == Endiannes::Middle {
            return Err(PackingError::BadOrdering(bitordering, "Value not allowed"));
        }
        if (bitoffset == 0 && data.len() < N) || (data.len() < N + 1) {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof <$t>",
            ));
        }

        data.copy_from_slice(self);

        #[cfg(target_endian = "big")]
        if bitordering == Endiannes::Little {
            data.reverse()
        };

        #[cfg(target_endian = "little")]
        if bitordering == Endiannes::Big {
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

    fn unpack_slice(
        src: &[u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<Self> {
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

        if bitordering == Endiannes::Little {
            data.reverse()
        };

        return Ok(data.try_into().unwrap());
    }
}

macro_rules! impl_pdudata {
    ($t: ty, $id: ident) => {
        impl PduData for $t {
            const PACKED_SIZE : usize = core::mem::size_of::<$t>();
            const ID: TypeId = TypeId::$id;

            /** Pack data in a slice according to the following argument: <br>
             * Data are packed following these step: ordering > offset > trunc
             - data: Packed data - "out" value<br>
             - bitoffset: Index of the data shift. Have to be between 0 and MAX_OFFSET (=7)<br>
             - bitsize: Size of the variable type: Integer on 16 bits => 16 or Integer on 3bits => 3 <br>
             - bitordering: Endian ordering for the packed data<br>

            ```mermaid
            graph TD;
                Input-->A[Get unsed bits];
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
            fn pack_slice(&self, data: &mut [u8], bitoffset: u8, bitsize: usize, bitordering: Endiannes) -> PackingResult<()>
            {
                //Test input parameters
                let array_len_bit : usize = data.len() * 8;
                let field_len_bit : usize = core::mem::size_of::<$t>() * 8;
                let excluded_bits : u8 = 8 - bitoffset;

                if bitsize == 0 {
                    return Err(PackingError::InvalidValue("Bit size couldn't be negative")); }
                if bitoffset >= MAX_OFFSET {
                    return Err(PackingError::BadAlignment(bitoffset as usize, "Offset greater than 8")); }
                if bitsize + bitoffset as usize > array_len_bit {
                    return Err(PackingError::BadSize(bitsize, "Data too large for the given slice")); }
                if bitoffset == 0 && data.len() < core::mem::size_of::<$t>() {
                    return Err(PackingError::InvalidValue("Data slice too short, must equal or greater than sizeof <$t>")); }

                //Get unsed data
                let prefix_mask : u8 = match bitoffset > 0 {
                    true => ( data[0] >> excluded_bits ) << excluded_bits,
                    false => 0 };

                //Ordering byte
                let ordering_bits = match bitordering {
                    Endiannes::Big => self.to_be_bytes(),
                    Endiannes::Little => self.to_le_bytes(),
                    _ => return Err(PackingError::BadOrdering(bitordering,"Mixed endian is invalid in this case"))
                };

                //Get suffix mask
                let suffix_mask : u8 = match bitoffset > 0 {
                    true => (ordering_bits[ordering_bits.len() -1] >> excluded_bits) << excluded_bits,
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
                for i in 0..data.len()
                {
                    if i == 0 {
                        data[i] = array[i] | prefix_mask;
                    }
                    else if i == array.len() && bitoffset > 0 {
                        data[i] = suffix_mask;
                    }
                    else if i < array.len() {
                        data[i] = array[i] ;
                    }
                }

                return Ok(());
            }

            fn unpack_slice(src: &[u8], bitoffset: u8, bitsize: usize, bitordering: Endiannes) -> PackingResult<Self> {
                //Test input parameters
                let array_len_bit = src.len() * 8;
                let field_len_bit = core::mem::size_of::<$t>() * 8;
                if bitoffset >= MAX_OFFSET {
                    return Err(PackingError::BadAlignment(bitoffset as usize, "Alignement superior than one byte")); }
                if bitsize + bitoffset as usize > array_len_bit || bitsize > field_len_bit {
                    return Err(PackingError::BadSize(bitsize, "Size greater than input slice size" )); }
                if (bitoffset == 0 && src.len() < core::mem::size_of::<$t>()) {
                    return Err(PackingError::InvalidValue("Data slice too short, must equal or greater than sizeof <$t>")); }

                //Extract data and suffix
                let mut data : $t = <$t>::from_be_bytes(src[0..core::mem::size_of::<$t>()].try_into().unwrap());
                data <<= bitoffset;
                let suffix : u8 = match src.len() > core::mem::size_of::<$t>() {
                    true => src[core::mem::size_of::<$t>()],
                    _ => 0,
                };

                //Concatenate suffix
                if bitsize + bitoffset as usize > field_len_bit {
                    let mut shift = suffix << (field_len_bit - bitsize - bitoffset as usize) as u8;
                    shift >>= (field_len_bit - bitsize - bitoffset as usize) as u8;
                    data |= shift as $t;
                }

                //Ordering
                return Ok(match bitordering {
                    Endiannes::Big => data,
                    Endiannes::Little => Self::from_le_bytes(data.to_be_bytes()),
                    _ => return Err(PackingError::BadOrdering(bitordering, "Mixed endian is invalid in this case"))
                });
            }
        }
    };
}

impl PduData for f32 {
    const PACKED_SIZE: usize = core::mem::size_of::<f32>();
    const ID: TypeId = TypeId::F32;

    fn pack_slice(
        &self,
        data: &mut [u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<()> {
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
            Endiannes::Little => self.to_le_bytes(),
            Endiannes::Big => self.to_be_bytes(),
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

    fn unpack_slice(
        src: &[u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<Self> {
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
            Endiannes::Little => f32::from_le_bytes(src.try_into().unwrap()),
            Endiannes::Big => f32::from_be_bytes(src.try_into().unwrap()),
            _ => {
                return Err(PackingError::BadOrdering(
                    bitordering,
                    "Mixed endian is invalid in this case",
                ))
            }
        });
    }
}

impl PduData for f64 {
    const PACKED_SIZE: usize = core::mem::size_of::<f64>();
    const ID: TypeId = TypeId::F64;

    fn pack_slice(
        &self,
        data: &mut [u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<()> {
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
            Endiannes::Little => self.to_le_bytes(),
            Endiannes::Big => self.to_be_bytes(),
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

    fn unpack_slice(
        src: &[u8],
        bitoffset: u8,
        bitsize: usize,
        bitordering: Endiannes,
    ) -> PackingResult<Self> {
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
            Endiannes::Little => f64::from_le_bytes(src.try_into().unwrap()),
            Endiannes::Big => f64::from_be_bytes(src.try_into().unwrap()),
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
    const PACKED_SIZE: usize = core::mem::size_of::<bool>();
    const ID: TypeId = TypeId::BOOL;

    fn pack_slice(
        &self,
        data: &mut [u8],
        bitoffset: u8,
        bitsize: usize,
        _bitordering: Endiannes,
    ) -> PackingResult<()> {
        if bitoffset >= MAX_OFFSET {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "Offset greater tan 8 excluded is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<bool>() {
            return Err(PackingError::BadSize(
                bitoffset as usize,
                "For this type,the size cannot be different than 1",
            ));
        }
        if bitoffset == 0 && data.len() != 1 {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof 1",
            ));
        }

        data[0] = u8::from(match self {
            true => 1,
            _ => 0,
        }) >> bitoffset;

        return Ok(());
    }

    fn unpack_slice(
        src: &[u8],
        bitoffset: u8,
        bitsize: usize,
        _bitordering: Endiannes,
    ) -> PackingResult<Self> {
        if bitoffset != 0 {
            return Err(PackingError::BadAlignment(
                bitoffset as usize,
                "For this type, offset different than 0 is not supported",
            ));
        }
        if bitsize != core::mem::size_of::<bool>() {
            return Err(PackingError::BadSize(
                bitoffset as usize,
                "For this type,the size cannot be different than 1",
            ));
        }
        if bitoffset == 0 && src.len() != 1 {
            return Err(PackingError::InvalidValue(
                "Data slice too short, must equal or greater than sizeof 1",
            ));
        }

        return Ok(src[0] << bitoffset == 1);
    }
}

impl_pdudata!(u8, U8);
impl_pdudata!(u16, U16);
impl_pdudata!(u32, U32);
impl_pdudata!(u64, U64);
impl_pdudata!(i8, I8);
impl_pdudata!(i16, I16);
impl_pdudata!(i32, I32);
impl_pdudata!(i64, I64);

/**
    locate some data in a datagram by its byte position and length, which must be extracted to type `T` to be processed in rust

    It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Clone)]
pub struct Field<T: PduData> {
    extracted: PhantomData<T>,
    /// start byte index of the object
    pub byte: usize,
    /// byte length of the object
    pub len: usize,
    /// define byte the alignement used by the field. Can be set only on "Ctor."
    pub endian: Endiannes,
}

impl<T: PduData> Field<T> {
    /// build a Field from its content
    pub fn new(byte: usize, len: usize, ordering: Endiannes) -> Self {
        Self {
            extracted: PhantomData,
            byte,
            len,
            endian: ordering,
        }
    }

    /// Build a field with default value:
    /// - 0 byte offset
    /// - 0 bit offset
    /// - Lenght = field type size
    /// - Little endian by ordering
    pub fn default() -> Self {
        Self {
            extracted: PhantomData,
            byte: 0,
            len: core::mem::size_of::<T>(),
            endian: Endiannes::Little,
        }
    }

    /// extract the value pointed by the field in the given byte array
    pub fn get(&self, data: &[u8]) -> PackingResult<T> {
        return T::unpack_slice(&data[self.byte..], 0, self.get_bits_len(), self.endian);
    }

    /// dump the given value to the place pointed by the field in the byte array
    pub fn set(&self, data: &mut [u8], value: T) -> PackingResult<()> {
        let mut sub_data = &mut data[self.byte..];
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
        if self.byte > 0 {
            write!(f, "Field{{ Size: {}, Endian: {} }}", self.len, self.endian) }
        else {
            write!(f, "Field{{ Byte: {}, Size: {}, Endian: {} }}", self.byte, self.len, self.endian) }

    }
}

impl<T: PduData> std::convert::TryFrom<BitField<T>> for Field<T> {
    type Error = PackingError;

    /// Comsusme a Bitfield struct and perform a transmutation into Field
    /// Caveat: If alignement is different thant 0 or size is not a multiple of 8 an error is throw.
    fn try_from(src : BitField<T>) -> Result<Self, Self::Error> {
        if src.bit != 0 {
            return Err(PackingError::BadAlignment(src.bit, "Bit alignement different from 0")); }
        return match src.len % 8 {
            0 => Ok(Self { extracted : src.extracted, byte : 0, len : src.len() / 8, endian : src.endian }),
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
    pub bit: usize,
    /// bit length of the object
    pub len: usize,
    /// define bit the ordering used by the field. Can be set only on "Ctor."
    pub endian: Endiannes,
}

impl<T: PduData> BitField<T> {
    /// build a Field from its content
    pub fn new(bit: usize, len: usize, endian: Endiannes) -> Self {
        Self {
            extracted: PhantomData,
            bit,
            len,
            endian,
        }
    }

    pub fn default() -> Self {
        Self {
            extracted: PhantomData,
            bit: 0,
            len: 0,
            endian: Endiannes::Big,
        }
    }

    /// extract the value pointed by the field in the given byte array
    pub fn get(&self, data: &[u8]) -> T {
        T::unpack_slice(&data[self.bit / 8 ..], (self.bit % 8) as u8, self.len, self.endian).expect("cannot unpack from data")
    }
    /// dump the given value to the place pointed by the field in the byte array
    pub fn set(&self, data: &mut [u8], value: T) {
        value.pack_slice(&mut data[self.bit / 8 ..], (self.bit % 8) as u8, self.len, self.endian);
    }

    /// Return field size in **bit**
    pub fn len(&self) -> usize {
        return self.len;
    }

}

impl<T: PduData> fmt::Debug for BitField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BitField{{ Bit: {}, Size: {},  Endian: {} }}", self.bit, self.len, self.endian)
    }
}

impl<T:PduData> std::convert::TryFrom<Field<T>> for BitField<T> {
    type Error = PackingError;

    /// Consume a Field struct and perfom a transmutation into Bitfield
    fn try_from(src : Field<T>) -> Result<Self, Self::Error> {
        if src.byte != 0 {
            return Err(PackingError::BadAlignment(src.byte, "Bit alignement different from 0")); }
        return Ok(Self { extracted: src.extracted, bit: 0, len: src.len() * 8, endian: src.endian });
    }
}
