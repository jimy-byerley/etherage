//! Traits and impls used to read/write data to/from the wire.

use core::{
	marker::PhantomData,
	fmt,
	};
pub use packed_struct::{PackingResult, PackingError, types::bits::ByteArray};

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
    type ByteArray: ByteArray;

    fn pack(&self) -> Self::ByteArray;
    fn unpack(src: &[u8]) -> PackingResult<Self>;
    
    fn packed_size() -> usize {Self::ByteArray::len()}
}
// /// trait marking a [packed_struct::PackedStruct] is a [PduData]
// // TODO: see if this trait could be derived
// pub trait PduStruct: packed::PackedStruct {}
// impl<T: PduStruct> PduData for T {
// 	const ID: TypeId = TypeId::CUSTOM;
// 	type ByteArray = <T as packed::PackedStruct>::ByteArray;
// 	
// 	fn pack(&self) -> Self::ByteArray    {packed::PackedStruct::pack(self).unwrap()}
// 	fn unpack(src: &[u8]) -> PackingResult<Self>  {packed::PackedStructSlice::unpack_from_slice(src)}
// }

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

impl<const N: usize> PduData for [u8; N] {
	const ID: TypeId = TypeId::CUSTOM;
	type ByteArray = Self;
	
	fn pack(&self) -> Self::ByteArray    {*self}
	fn unpack(src: &[u8]) -> PackingResult<Self>  {
		Ok(Self::try_from(src)
			.map_err(|_| PackingError::BufferSizeMismatch{
				expected: N, 
				actual: src.len(),
				})?
			.clone())
	}
}

impl PduData for bool {
	const ID: TypeId = TypeId::BOOL;
	type ByteArray = [u8; 1];
	
	fn pack(&self) -> Self::ByteArray    {
        Self::ByteArray::new(if *self {0b1} else {0b0})
	}
	fn unpack(src: &[u8]) -> PackingResult<Self>  {
		Ok(src[0] & 0b1 == 0b1)
	}
}

/// macro implementing [PduData] for a given struct generated with `bilge`
macro_rules! bilge_pdudata {
    ($t: ty) => { crate::data::packed_pdudata!($t); }
//     ($t: ty, $id: ident) => { impl PduData for $t {
//         const ID: TypeId = TypeId::CUSTOM;
//         type ByteArray = [u8; ($id::BITS as usize + 7)/8];
//         
//         fn pack(&self) -> PackingResult<Self::ByteArray> {
//             Ok($id::from(*self).to_le_bytes())
//         }
//         fn unpack(src: &[u8]) -> PackingResult<Self> {
//             Ok(Self::from($id::from_le_bytes(src.try_into().map_err(|_|
//                 PackingError::BufferSizeMismatch{
//                     expected: ($id::BITS as usize + 7)/8,
//                     actual: src.len(),
//                 })?.clone()
//                 )))
//         }
//     }};
}
pub(crate) use bilge_pdudata;

/// unsafe macro implementing [PduData] for a given struct with `repr(packed)`
macro_rules! packed_pdudata {
    ($t: ty) => { impl crate::data::PduData for $t {
        const ID: crate::data::TypeId = crate::data::TypeId::CUSTOM;
        type ByteArray = [u8; core::mem::size_of::<$t>()];
        
        fn pack(&self) -> Self::ByteArray {
            unsafe{ core::mem::transmute::<Self, Self::ByteArray>(self.clone()) }
        }
        fn unpack(src: &[u8]) -> crate::data::PackingResult<Self> {
            let src: &Self::ByteArray = src.try_into().map_err(|_|
                crate::data::PackingError::BufferSizeMismatch{
                    expected: Self::ByteArray::len(),
                    actual: src.len(),
                })?;
            Ok(unsafe{ core::mem::transmute::<Self::ByteArray, Self>(src.clone()) })
        }
    }};
}
pub(crate) use packed_pdudata;

/// macro implementing [PduData] for numeric types
macro_rules! num_pdudata {
	($t: ty, $id: ident) => { impl crate::data::PduData for $t {
			const ID: crate::data::TypeId = crate::data::TypeId::$id;
			type ByteArray = [u8; core::mem::size_of::<$t>()];
			
			fn pack(&self) -> Self::ByteArray {
				self.to_le_bytes()
			}
			fn unpack(src: &[u8]) -> crate::data::PackingResult<Self> {
				Ok(Self::from_le_bytes(src
					.try_into()
					.map_err(|_|  crate::data::PackingError::BufferSizeMismatch{
								expected: core::mem::size_of::<$t>(),
								actual: src.len(),
								})?
					))
			}
		}};
	($t: ty) => { num_pdudata!(t, crate::data::TypeId::CUSTOM) };
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


// /// much like PduData but works with variable size types, and allows partial decoding of data.
// /// [Self::unpack] however returns data limited to the lifetime of the unpacked buffer.
// pub trait FrameData<'a>: Sized {
//     fn packed_size(&self) -> usize;
//     fn pack(&self, dst: &mut [u8]) -> PackingResult<()>;
//     fn unpack(src: &'a [u8]) -> PackingResult<Self>;
// }
// 
// impl<'a> FrameData<'a> for &'a [u8] {
//     fn packed_size(&self) -> usize {self.len()}
//     fn pack(&self, dst: &mut [u8]) -> PackingResult<()> {Ok(dst.copy_from_slice(self))}
//     fn unpack(src: &'a [u8]) -> PackingResult<Self> {Ok(src)}
// }

// impl<T: PduData> FrameData<'a> for T {
//     fn packed_size(&self) -> usize {<Self as PduData>::packed_size()}
//     fn pack(&self, dst: &mut [u8]) -> PackingResult<()> {
//         dst[.. self.packed_size()].copy_from_slice(<Self as PduData>::pack(self).as_bytes_slice());
//         Ok(())
//     }
//     fn unpack(src: &'a [u8]) -> PackingResult<Self> {
//         <Self as PduData>::unpack(src)
//     }
// }


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
}
impl<T: PduData> Field<T>
{
	/// build a Field from its byte offset and byte length
	pub const fn new(byte: usize, len: usize) -> Self {
		Self{extracted: PhantomData, byte, len}
	}
	/// build a Field from its byte offset, infering its length from the data nominal size
	pub const fn simple(byte: usize) -> Self {
        Self{extracted: PhantomData, byte, len: T::ByteArray::len()}
	}
	/// extract the value pointed by the field in the given byte array
	pub fn get(&self, data: &[u8]) -> T       {
		T::unpack(&data[self.byte..][..self.len])
				.expect("cannot unpack from data")
	}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, data: &mut [u8], value: T)   {
		data[self.byte..][..self.len].copy_from_slice(
			value.pack().as_bytes_slice()
			);
	}
}
impl<T: PduData> fmt::Debug for Field<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Field{{{}, {}}}", self.byte, self.len)
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
}
impl<T: PduData> BitField<T> {
	/// build a Field from its content
	pub fn new(bit: usize, len: usize) -> Self {
		Self{extracted: PhantomData, bit, len}
	}
	/// extract the value pointed by the field in the given byte array
	pub fn get(&self, _data: &[u8]) -> T       {todo!()}
	/// dump the given value to the place pointed by the field in the byte array
	pub fn set(&self, _data: &mut [u8], _value: T)   {todo!()}
}
impl<T: PduData> fmt::Debug for BitField<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "BitField{{{}, {}}}", self.bit, self.len)
	}
}




pub struct Cursor<T> {
    position: usize,
    data: T,
}
impl<T> Cursor<T> {
    pub fn new(data: T) -> Self   {Self{position: 0, data}}
    pub fn position(&self) -> usize   {self.position}
}
impl<'a> Cursor<&'a [u8]> {
    pub fn unpack<T: PduData>(&mut self) -> PackingResult<T> {
        let start = self.position.clone();
        self.position += T::packed_size();
        T::unpack(&self.data[start .. self.position])
    }
    pub fn read(&mut self, size: usize) -> PackingResult<&'a [u8]> {
        let start = self.position.clone();
        self.position += size;
        Ok(&self.data[start .. self.position])
    }
    pub fn remain(&self) -> &'a [u8] {
        &self.data[self.position ..]
    }
    pub fn finish(self) -> &'a [u8] {
        &self.data[.. self.position]
    }
}
impl<'a> Cursor<&'a mut [u8]> {
    pub fn unpack<T: PduData>(&mut self) -> PackingResult<T> {
        let start = self.position.clone();
        self.position += T::packed_size();
        T::unpack(&self.data[start .. self.position])
    }
    pub fn read(&mut self, size: usize) -> PackingResult<&'a [u8]> {
        let start = self.position.clone();
        self.position += size;
        Ok(&self.data[start .. self.position])
    }
    pub fn pack<T: PduData>(&mut self, value: &T) -> PackingResult<()> {
        let start = self.position.clone();
        self.position += T::packed_size();
        self.data[start .. self.position].copy_from_slice(value.pack().as_bytes_slice());
        Ok(())
    }
    pub fn write(&mut self, value: &[u8]) -> PackingResult<()> {
        let start = self.position.clone();
        self.position += value.len();
        self.data[start .. self.position].copy_from_slice(value);
        Ok(())
    }
    pub fn remain<'b>(&'b mut self) -> &'b mut [u8] {
        &mut self.data[self.position ..]
    }
    pub fn finish(self) -> &'a mut [u8] {
        &mut self.data[.. self.position]
    }
}
