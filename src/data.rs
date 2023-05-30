//! Traits and impls used to read/write data to/from the wire.

use core::{fmt, marker::PhantomData};
use packed_struct as packed;
pub use packed_struct::{types::bits::ByteArray, PackingError, PackingResult};

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
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BitOrdering {
    Little,
    Big,
    Middle,
}

/** Implement default value for bits ordering as LE - little endian */
impl Default for BitOrdering {
    fn default() -> Self {
        Self::Little
    }
}

type PackingResult<T> = Result<T, PackingError>;
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
	const MAX_OFFSET : u8 = 8;
    type ByteArray: ByteArray;

    fn pack(&self, align: BitOrdering, offset: u8) -> Self::ByteArray;
    fn unpack(src: &[u8], align: BitOrdering, offset: u8) -> PackingResult<Self>;
    fn pack_default(&self) -> Self::ByteArray { self.pack(BitOrdering::Little, 0) }
    fn unpack_default(src: &[u8]) -> PackingResult<Self> {  Self::unpack(src, BitOrdering::Little, 0) }
}

/// trait marking a [packed_struct::PackedStruct] is a [PduData]
// TODO: see if this trait could be derived
pub trait PduStruct: packed::PackedStruct {}

impl<T: PduStruct> PduData for T {
    const ID: TypeId = TypeId::CUSTOM;
    type ByteArray = <T as packed::PackedStruct>::ByteArray;

    fn pack(&self, align: BitOrdering, offset: u8) -> Self::ByteArray {
		let array = packed::PackedStruct::pack(self).unwrap();
        let mut buffer: Vec<u8> =  array.as_bytes_slice().to_vec();
        for item in buffer.iter_mut() {
            match align {
                BitOrdering::Little => *item += 0xFF,
                _ => (),
            };
        }
		return array;
    }

    fn unpack(src: &[u8], align: BitOrdering, offset: u8) -> PackingResult<Self> {
        packed::PackedStructSlice::unpack_from_slice(src)
    }
}

impl<const N: usize> PduData for [u8; N] {
    const ID: TypeId = TypeId::CUSTOM;
    type ByteArray = Self;

    fn pack(&self, align: BitOrdering, offset: u8) -> Self::ByteArray {
		if offset >= Self::MAX_OFFSET {
			unimplemented!(); }

        let mut data: [u8; N] = Self::ByteArray::new(0);
        data.copy_from_slice(self);

        #[cfg(target_endian = "big")]
		if align == BitOrdering::Little {
			data.reverse() };

        #[cfg(target_endian = "little")]
        if align == BitOrdering::Big {
			data.reverse() };

		for i in N..1 {
			data.swap(N - i - 1, N - i - 2)
		}

		let mut i = N - 1;
		while i > 0 {
			if i < offset { data[i] = 0;}
            else { data[i] = data[i - offset as usize]; }
			i -= 1;
		}

        *self
    }

    fn unpack(src: &[u8], align: BitOrdering, offset: u8) -> PackingResult<Self> {

		if offset >= Self::MAX_OFFSET {
			unimplemented!(); }

        let mut data = Self::try_from(src)
            .map_err(|_| PackingError::BufferSizeMismatch { expected: N, actual: src.len(),})?
            .clone();

		if align == BitOrdering::Little {
			data.reverse() };

		for i in N..1 {
			data.swap(N - i - 1, N - i - 2)
		}

		let mut i : usize = N- 1;
		while i > 0 {
			if i < offset { data[i] = 0;}
            else { data[i] = data[i - offset as usize]; }
			i -= 1;
		}

		return  Ok(data);
    }
}

macro_rules! impl_pdudata {
    ($t: ty, $id: ident) => {
        impl PduData for $t {
            const ID: TypeId = TypeId::$id;
            type ByteArray = [u8; core::mem::size_of::<$t>()];

            fn pack(&self, align: Ordering, offset: u8) -> Self::ByteArray {
                //Ordering byte
                let data_shift = match align {
                    Ordering::BE => self.to_be_bytes(),
                    Ordering::LE => self.to_le_bytes(),
                    _ => unimplemented!(),
                };

                //Data shift
				let mut data: Self::ByteArray = Self::ByteArray::new(0);
				if offset < Self::MAX_OFFSET {
					let mut shrinked: u8 = 0;
					let ioffset: u8 = 8 - offset;

					for i in 0..core::mem::size_of::<$t>() - 1 {
						if i < core::mem::size_of::<$t>() - 2 {
                            data[i + 1] += shrinked; }	// Add previous shifted data (if exist)
						shrinked = data_shift[i] << ioffset; 			// Get overlapped part [n ... x] -> [x ... 0]
						data[i] = data_shift[i] >>  offset;	  	// Shifting
					}
				}

                return data;
            }

            fn unpack(src: &[u8], align: Ordering, offset: u8) -> PackingResult<Self> {

                // //Bind data
                // let data = src.try_into().map_err(|_| PackingError::BufferSizeMismatch {
                //         expected: core::mem::size_of::<$t>(),
                //         actual: src.len(),
                //     })?;

                //Shift bit
                let mut data_shift: Self::ByteArray = Self::ByteArray::new(0);
                if offset < Self::MAX_OFFSET {
                    let mut shrinked: u8 = 0;
                    let ioffset: u8 = 8 - offset;

                    for i in 0..src.len() - 1 {
                        if i < src.len() - 2 {
                            data_shift[i + 1] += shrinked; }	    // Add previous shifted data (if exist)
                        shrinked = src[i] << ioffset; 	    // Get overlapped part [n ... x] -> [x ... 0]
                        data_shift[i] = src[i] >>  offset;  // Shifting
                    }
                }

                //Return value based on endianes
                return Ok(match align {
                    Ordering::BE => Self::from_be_bytes(data_shift),
                    Ordering::LE => Self::from_le_bytes(data_shift),
                    _ => unimplemented!()
                });
            }
        }
    };
    ($t: ty) => {
        impl_pdudata_float(t, TypeId::CUSTOM)
    };
}

impl_pdudata!(u8, U8);
impl_pdudata!(u16, U16);
impl_pdudata!(u32, U32);
impl_pdudata!(u64, U64);
impl_pdudata!(i8, I8);
impl_pdudata!(i16, I16);
impl_pdudata!(i32, I32);
impl_pdudata!(i64, I64);
impl_pdudata!(f32, F32);
impl_pdudata!(f64, F64);

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
    /// define bit the alignement used by the field. Can be set only on "Ctor."
    align: BitOrdering,
    /// shift *in bits* used to read data
    /// Bits: 	| 0 1 2 3 4 5 6 8 0 1 2 3 4 5 6 7 0 1 2 3 ...|
    /// Bytes:	| 0 0 0 0 0 0 0 0 1 1 1 1 1 1 1 1 2 2 2 2 ...|
    /// Data:	| * * * 0 0 0 0 0 0 0 0 1 1 1 1 1 1 1 1 2 ...|
    offset: u8,
}

impl<T: PduData> Field<T> {
    /// build a Field from its content
    pub fn new(byte: usize, len: usize, align: BitOrdering, offset: u8) -> Self {
        Self {
            extracted: PhantomData,
            byte,
            len,
            align,
            offset,
        }
    }
    /// extract the value pointed by the field in the given byte array
    pub fn get(&self, data: &[u8]) -> T {
        T::unpack(&data[self.byte..][..self.len], self.align, self.offset)
            .expect("cannot unpack from data")
    }
    /// dump the given value to the place pointed by the field in the byte array
    pub fn set(&self, data: &mut [u8], value: T) {
        data[self.byte..][..self.len]
            .copy_from_slice(value.pack(self.align, self.offset).as_bytes_slice());
    }
    fn get_alignement(&self) -> BitOrdering {
        self.align
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
    /// define bit the ordering used by the field. Can be set only on "Ctor."
    align: BitOrdering,
    /// shift *in bits* used to read data
    /// Bits: 	| 0 1 2 3 4 5 6 8 0 1 2 3 4 5 6 7 0 1 2 3 ...|
    /// Bytes:	| 0 0 0 0 0 0 0 0 1 1 1 1 1 1 1 1 2 2 2 2 ...|
    /// Data:	| * * * 0 0 0 0 0 0 0 0 1 1 1 1 1 1 1 1 2 ...|
    offset: u8,
}

impl<T: PduData> BitField<T> {
    /// build a Field from its content
    pub fn new(bit: usize, len: usize, align: BitOrdering, offset: u8) -> Self {
        Self {
            extracted: PhantomData,
            bit,
            len,
            align,
            offset,
        }
    }
    /// extract the value pointed by the field in the given byte array
    pub fn get(&self, _data: &[u8]) -> T {
        todo!()
    }
    /// dump the given value to the place pointed by the field in the byte array
    pub fn set(&self, _data: &mut [u8], _value: T) {
        todo!()
    }
}

impl<T: PduData> fmt::Debug for BitField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BitField{{{}, {}}}", self.bit, self.len)
    }
}