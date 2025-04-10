//! Traits and impls used to read/write data to/from the wire.

use core::{
    marker::PhantomData,
    fmt,
    };

/**
    trait for data types than can be packed/unpacked to/from a PDU
*/
pub trait PduData: Sized {
    /// identifier for this data type (according to ESI file)
    const ID: TypeId;
    /// byte array for serializing this data
    type Packed: Storage;

    /// serialize to a byte slice (the slice might be bigger than actually needed)
    fn pack(&self, dst: &mut [u8]) -> PackingResult<()>;
    /// deserialize from a byte slice (the slice might be bigger than actually needed)
    fn unpack(src: &[u8]) -> PackingResult<Self>;

    /// convenient getter for [Self::Packed::LEN]
    fn packed_size() -> usize  {Self::Packed::LEN}
    /// convenient getter for [Self::Packed::LEN] * 8
    fn packed_bitsize() -> usize {Self::Packed::LEN*8}
    
    /// like [Self::pack] but to a byte array instead of a slice
    fn packed(&self) -> PackingResult<Self::Packed> {
        let mut buffer = Self::Packed::uninit();
        self.pack(buffer.as_mut())?;
        Ok(buffer)
    }
}

/**
    Enum to identify and raise adapted error raised by this package
*/
#[derive(Copy, Clone, Debug)]
pub enum PackingError {
    BadSize(usize, &'static str),
    BadAlignment(usize, &'static str),
//     BadOrdering(BitOrdering, &'static str),
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
//     fn zeroed() -> Self;
}
impl<const N: usize> Storage for [u8; N] {
    const LEN: usize = N;
    fn uninit() -> Self {unsafe {core::mem::uninitialized()}}
//     fn zeroed() -> Self {unsafe {core::mem::zeroed()}}
}

/**
    dtype identifiers associated to dtypes allowing to dynamically check the type of a [PduData] implementor

    It is only convering the common useful types and not all the possible implementors of [PduData]
*/
#[derive(Copy, Clone, Debug)]
pub enum TypeId {
    /// default value of the enum, used in case the matching [PduData] does not fit in any of these integers
    CUSTOM,
    VOID, BOOL,
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    F32, F64,
}

impl<const N: usize> PduData for [u8; N] {
    const ID: TypeId = TypeId::CUSTOM;
    type Packed = Self;

    fn pack(&self, dst: &mut [u8]) -> PackingResult<()> {
        dst.copy_from_slice(self);
        Ok(())
    }
    fn unpack(src: &[u8]) -> PackingResult<Self>  {
        if src.len() < Self::Packed::LEN
            {return Err(PackingError::BadSize(src.len(), "not enough bytes for desired slice"))}
        Ok(Self::try_from(&src[.. Self::Packed::LEN]).unwrap().clone())
    }
}

impl PduData for () {
    const ID: TypeId = TypeId::VOID;
    type Packed = [u8; 0];

    fn pack(&self, _dst: &mut [u8]) -> PackingResult<()>  {Ok(())}
    fn unpack(_src: &[u8]) -> PackingResult<Self>  {Ok(())}
}

impl PduData for bool {
    const ID: TypeId = TypeId::BOOL;
    type Packed = [u8; 1];

    fn pack(&self, dst: &mut [u8]) -> PackingResult<()>  {
        if dst.len() < Self::Packed::LEN
            {return Err(PackingError::BadSize(dst.len(), ""))}
        dst[0] = if *self {0b1} else {0b0};
        Ok(())
    }
    fn unpack(src: &[u8]) -> PackingResult<Self>  {
        if src.len() < Self::Packed::LEN
            {return Err(PackingError::BadSize(src.len(), ""))}
        Ok(src[0] & 0b1 == 0b1)
    }
}

/// macro implementing [PduData] for a given struct generated with `bilge`
/// this is an ugly unsafe code to overcome the lack of traits providing containing ints in [bilge]
#[macro_export]
macro_rules! bilge_pdudata {
    ($t: ty, $id: ident) => {  impl $crate::data::PduData for $t {
        // we cannot use Self.value for unknown reason
        // we cannot use $id::from(Self) for unknown reason
        // we cannot use $id::to_le_bytes because it is only implemented for byte exact integers

        const ID: $crate::data::TypeId = $crate::data::TypeId::CUSTOM;
        type Packed = [u8; ($id::BITS as usize + 7)/8];

        fn pack(&self, dst: &mut [u8]) -> $crate::data::PackingResult<()> {
            use $crate::data::Storage;
            if dst.len() < Self::Packed::LEN
                {return Err($crate::data::PackingError::BadSize(dst.len(), "bilge struct needs exact size"))}
                
            let common = Self::Packed::LEN.min(core::mem::size_of::<Self>());
            let src = unsafe{ core::mem::transmute::<&Self, &Self::Packed>(self) };
            dst[.. common].copy_from_slice(&src[.. common]);
            dst[common .. Self::Packed::LEN].fill(0);
            Ok(())
        }
        fn unpack(src: &[u8]) -> $crate::data::PackingResult<Self> {
            use $crate::data::Storage;
            if src.len() < Self::Packed::LEN
                {return Err($crate::data::PackingError::BadSize(src.len(), "bilge struct needs exact size"))}
            
            let mut tmp = [0; core::mem::size_of::<Self>()];
            let common = Self::Packed::LEN.min(core::mem::size_of::<Self>());
            tmp[.. common].copy_from_slice(&src[.. common]);
            Ok(unsafe{ core::mem::transmute::<[u8; core::mem::size_of::<Self>()], Self>(tmp) })
        }
    }};
}
pub use bilge_pdudata;

/// unsafe macro implementing [PduData] for a given struct with `repr(packed)`
#[macro_export]
macro_rules! packed_pdudata {
    ($t: ty) => { impl $crate::data::PduData for $t {
        const ID: $crate::data::TypeId = $crate::data::TypeId::CUSTOM;
        type Packed = [u8; core::mem::size_of::<$t>()];

        fn pack(&self, dst: &mut [u8]) -> $crate::data::PackingResult<()> {
            use $crate::data::Storage;
            if dst.len() < Self::Packed::LEN
                {return Err($crate::data::PackingError::BadSize(dst.len(), "not enough bytes for struct"))}
            dst[..Self::Packed::LEN].copy_from_slice(&unsafe{ core::mem::transmute_copy::<Self, Self::Packed>(self) });
            Ok(())
        }
        fn unpack(src: &[u8]) -> $crate::data::PackingResult<Self> {
            use $crate::data::Storage;
            if src.len() < Self::Packed::LEN
                {return Err($crate::data::PackingError::BadSize(src.len(), "not enough bytes for struct"))}
            let src: &Self::Packed = src[.. Self::Packed::LEN].try_into().unwrap();
            Ok(unsafe{ core::mem::transmute::<Self::Packed, Self>(src.clone()) })
        }
    }};
}
pub use packed_pdudata;

/// unsafe macro implementing [PduData] for arrays of a given struct with `repr(packed)`
#[macro_export]
macro_rules! array_pdudata {
    ($t: ty, $n: literal, $($rest: literal),+) => {
		packed_pdudata!([$t; $n]); 
		array_pdudata!($t, $($rest),+);
	};
    ($t: ty, $n: literal) => {
		packed_pdudata!([$t; $n]); 
	};
    ($t: ty) => {array_pdudata!($t, 0, 1, 2, 3, 4, 5, 6, 7, 8);};
}
pub use array_pdudata;

/// macro implementing [PduData] for numeric types
macro_rules! num_pdudata {
    ($t: ty, $id: ident) => { impl $crate::data::PduData for $t {
            const ID: $crate::data::TypeId = $crate::data::TypeId::$id;
            type Packed = [u8; core::mem::size_of::<$t>()];

            fn pack(&self, dst: &mut [u8]) -> $crate::data::PackingResult<()> {
                dst.copy_from_slice(&self.to_le_bytes());
                Ok(())
            }
            fn unpack(src: &[u8]) -> $crate::data::PackingResult<Self> {
                Ok(Self::from_le_bytes(src
                    .try_into()
                    .map_err(|_|  $crate::data::PackingError::BadSize(src.len(), "integger cannot be yet zeroed"))?
                    ))
            }
        }};
    ($t: ty) => { num_pdudata!(t, $crate::data::TypeId::CUSTOM) };
}

num_pdudata!(u8, U8);
num_pdudata!(u16, U16);
num_pdudata!(u32, U32);
num_pdudata!(u64, U64);
num_pdudata!(i8, I8);
num_pdudata!(i16, I16);
num_pdudata!(i32, I32);
num_pdudata!(i64, I64);
num_pdudata!(f32, F32);
num_pdudata!(f64, F64);

array_pdudata!(u16);
array_pdudata!(u32);
array_pdudata!(u64);
array_pdudata!(i8);
array_pdudata!(i16);
array_pdudata!(i32);
array_pdudata!(i64);
array_pdudata!(f32);
array_pdudata!(f64);

/**
    Types whose instances can be read from and written to a PDU. 
    
    It differs from [PduData] which is able to be read to and written from PDU noly using their type static definitions
*/
pub trait PduField<T> {
    /// extract the value pointed by the field in the given byte array
    fn get(&self, data: &[u8]) -> T;
    /// dump the given value to the place pointed by the field in the byte array
    fn set(&self, data: &mut [u8], value: T);
}

/// convenient alias, recommended to use everywhere it is obvious fields are [ByteField]
pub type Field<T> = ByteField<T>;

/**
    locate some data in a datagram by its byte position and length, which must be extracted to type `T` to be processed in rust

    It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Eq, Hash)]
pub struct ByteField<T: PduData> {
    /// this is only here to mark that T is actually used
    extracted: PhantomData<T>,
    /// start byte index of the object
    pub byte: usize,
    /// byte length of the object
    pub len: usize,
}
impl<T: PduData> ByteField<T>
{
    /// build a Field from its byte offset and byte length
    pub const fn new(byte: usize, len: usize) -> Self {
        Self{extracted: PhantomData, byte, len}
    }
    /// build a Field from its byte offset, infering its length from the data nominal size
    pub const fn simple(byte: usize) -> Self {
        Self{extracted: PhantomData, byte, len: T::Packed::LEN}
    }
    pub const fn downcast(&self) -> ByteField<()> {
        ByteField {extracted: PhantomData, byte: self.byte, len: self.len}
    }
}
impl<T: PduData> PduField<T> for ByteField<T> {
    /// extract the value pointed by the field in the given byte array
    fn get(&self, data: &[u8]) -> T       {
        T::unpack(&data[self.byte..][..self.len])
            .expect("cannot unpack from data")
    }
    /// dump the given value to the place pointed by the field in the byte array
    fn set(&self, data: &mut [u8], value: T)   {
        value.pack(&mut data[self.byte..][..self.len])
            .expect("cannot pack data")
    }
}
impl<T: PduData> fmt::Debug for ByteField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Field<{}>{{0x{:x}, {}}}", core::any::type_name::<T>(), self.byte, self.len)
    }
}
// [Clone], [Copy] and [PartialEq] must be implemented manually to allow copying a field pointing to a type which does not implement this operation
impl<T: PduData> Clone for ByteField<T> {
    fn clone(&self) -> Self   {Self::new(self.byte, self.len)}
}
impl<T: PduData> Copy for ByteField<T> {}
impl<T: PduData> PartialEq for ByteField<T> {
    fn eq(&self, other: &Self) -> bool {
        self.byte == other.byte && self.len == other.len
    }
}


/**
    locate some data in a datagram by its bit position and length, which must be extracted to type `T` to be processed in rust

    It acts like a getter/setter of a value in a byte sequence. One can think of it as an offset to a data location because it does not actually point the data but only its offset in the byte sequence, it also contains its length to dynamically check memory bounds.
*/
#[derive(Default, Eq, Hash)]
pub struct BitField<T: PduData> {
    /// this is only here to mark that T is actually used
    extracted: PhantomData<T>,
    /// start bit index of the object
    pub bit: usize,
    /// bit length of the object
    pub len: usize,
}
impl<T: PduData> BitField<T> {
    /// build a Field from its content
    pub const fn new(bit: usize, len: usize) -> Self {
        Self{extracted: PhantomData, bit, len}
    }
    pub const fn downcast(&self) -> BitField<()> {
        BitField {extracted: PhantomData, bit: self.bit, len: self.len}
    }
}
impl<T: PduData> PduField<T> for BitField<T> {
    /// extract the value pointed by the field in the given byte array
    fn get(&self, data: &[u8]) -> T {
        // TODO: support bigger types
        assert!(self.len < 128);
        let mut buf = [0u8; 16];
        // TODO: process word by word instead of bit by bit
        for i in 0 .. self.len {
            let bufbyte = i/8;
            let bufbit = i%8;
            let databyte = (self.bit + i)/8;
            let databit = (self.bit + i)%8;
            buf[bufbyte] |= ((data[databyte] >> databit) & 1) << bufbit;
        }
        T::unpack(&buf)
            .expect("cannot unpack from intermediate buffer")
    }
    /// dump the given value to the place pointed by the field in the byte array
    fn set(&self, data: &mut [u8], value: T)   {
        // TODO: support bigger types
        assert!(self.len < 128);
        let mut buf = [0u8; 16];
        value.pack(&mut buf)
            .expect("cannot pack to intermediate buffer");
        // TODO: process word by word instead of bit by bit
        for i in 0 .. self.len {
            let bufbyte = i/8;
            let bufbit = i%8;
            let databyte = (self.bit + i)/8;
            let databit = (self.bit + i)%8;
            data[databyte] &= !(1 << databit);
            data[databyte] |= ((buf[bufbyte] >> bufbit) & 1) << databit;
        }
    }
}
impl<T: PduData> fmt::Debug for BitField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BitField<{}>{{{}, {}}}", core::any::type_name::<T>(), self.bit, self.len)
    }
}
// [Clone], [Copy] and [PartialEq] must be implemented manually to allow copying a field pointing to a type which does not implement this operation
impl<T: PduData> Clone for BitField<T> {
    fn clone(&self) -> Self   {Self::new(self.bit, self.len)}
}
impl<T: PduData> Copy for BitField<T> {}
impl<T: PduData> PartialEq for BitField<T> {
    fn eq(&self, other: &Self) -> bool {
        self.bit == other.bit && self.len == other.len
    }
}



/**
    helper to read/write sequencial data from/to a byte slice

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
    /**
        current position in the read/write slice

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
