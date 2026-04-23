use core::mem;

use no_std_io::io::{Read, Seek, Write};

use heapless::{LenType, Vec};

use crate::{deku_error, reader::Reader};
use crate::writer::Writer;
use crate::{ctx::*, DekuReader};
use crate::{DekuError, DekuWriter};

impl<const N: usize, LenT: LenType> DekuReader<'_, ReadExact> for Vec<u8, N, LenT> {
    fn from_reader_with_ctx<R: Read + Seek>(
        reader: &mut Reader<R>,
        exact: ReadExact,
    ) -> Result<Self, DekuError>
    where
        Self: Sized,
    {
        let mut bytes = Vec::from_array([0; N]);
        bytes.truncate(exact.0);
        let _ = reader.read_bytes(exact.0, &mut bytes, Order::Lsb0)?;
        Ok(bytes)
    }
}

/// Read `T`s into a vec until a given predicate returns true
/// * `ctx` - The context required by `T`. It will be passed to every `T` when constructing.
/// * `predicate` - the predicate that decides when to stop reading `T`s
///   The predicate takes two parameters: the number of bits that have been read so far,
///   and a borrow of the latest value to have been read. It should return `true` if reading
///   should now stop, and `false` otherwise
fn reader_vec_with_predicate<'a, T, const N: usize, LenT: LenType, Ctx, Predicate, R: Read + Seek>(
    reader: &mut Reader<R>,
    ctx: Ctx,
    mut predicate: Predicate,
) -> Result<Vec<T, N, LenT>, DekuError>
where
    T: DekuReader<'a, Ctx>,
    Ctx: Copy,
    Predicate: FnMut(usize, &T) -> bool,
{
    // ZST detected, return empty vec
    if mem::size_of::<T>() == 0 {
        return Ok(Vec::new());
    }

    let mut res = Vec::new();

    let start_read = reader.bits_read;

    loop {
        let val = <T>::from_reader_with_ctx(reader, ctx)?;
        res.push(val).map_err(|_| deku_error!(DekuError::Parse, "heapless vec at capacity", "{}", N))?;

        // This unwrap is safe as we are pushing to the vec immediately before it,
        // so there will always be a last element
        if predicate(reader.bits_read - start_read, res.last().unwrap()) {
            break;
        }
    }

    Ok(res)
}

fn reader_vec_to_end<'a, T, const N: usize, LenT: LenType, Ctx, R: Read + Seek>(
    reader: &mut crate::reader::Reader<R>,
    ctx: Ctx,
) -> Result<Vec<T, N, LenT>, DekuError>
where
    T: DekuReader<'a, Ctx>,
    Ctx: Copy,
{
    // ZST detected, return empty vec
    if mem::size_of::<T>() == 0 {
        return Ok(Vec::new());
    }

    let mut res = Vec::new();
    loop {
        if reader.end() {
            break;
        }
        let val = <T>::from_reader_with_ctx(reader, ctx)?;
        res.push(val).map_err(|_| deku_error!(DekuError::Parse, "heapless vec at capacity", "{}", N))?;
    }

    Ok(res)
}

impl<'a, T, const N: usize, LenT: LenType, Ctx, Predicate> DekuReader<'a, (Limit<T, Predicate>, Ctx)> for Vec<T, N, LenT>
where
    T: DekuReader<'a, Ctx>,
    Ctx: Copy,
    Predicate: FnMut(&T) -> bool,
{
    fn from_reader_with_ctx<R: Read + Seek>(
        reader: &mut Reader<R>,
        (limit, inner_ctx): (Limit<T, Predicate>, Ctx),
    ) -> Result<Self, DekuError>
    where
        Self: Sized,
    {
        match limit {
            // Read a given count of elements
            Limit::Count(mut count) => {
                // Handle the trivial case of reading an empty vector
                if count == 0 {
                    return Ok(Vec::new());
                }

                // Otherwise, read until we have read `count` elements
                reader_vec_with_predicate(reader, inner_ctx, move |_, _| {
                    count -= 1;
                    count == 0
                })
            }

            // Read until a given predicate returns true
            Limit::Until(mut predicate, _) => {
                reader_vec_with_predicate(reader, inner_ctx, move |_, value| predicate(value))
            }

            // Read until a given quantity of bits have been read
            Limit::BitSize(size) => {
                let bit_size = size.0;

                // Handle the trivial case of reading an empty vector
                if bit_size == 0 {
                    return Ok(Vec::new());
                }

                reader_vec_with_predicate(reader, inner_ctx, move |read_bits, _| {
                    read_bits == bit_size
                })
            }

            // Read until a given quantity of bytes have been read
            Limit::ByteSize(size) => {
                let bit_size = size.0 * 8;

                // Handle the trivial case of reading an empty vector
                if bit_size == 0 {
                    return Ok(Vec::new());
                }

                reader_vec_with_predicate(reader, inner_ctx, move |read_bits, _| {
                    read_bits == bit_size
                })
            }

            Limit::End => reader_vec_to_end(reader, inner_ctx),
        }
    }
}

impl<'a, T: DekuReader<'a>, const N: usize, LenT: LenType, Predicate: FnMut(&T) -> bool> DekuReader<'a, Limit<T, Predicate>>
    for Vec<T, N, LenT>
{
    /// Read `T`s until the given limit from input for types which don't require context.
    fn from_reader_with_ctx<R: Read + Seek>(
        reader: &mut Reader<R>,
        limit: Limit<T, Predicate>,
    ) -> Result<Self, DekuError>
    where
        Self: Sized,
    {
        Vec::from_reader_with_ctx(reader, (limit, ()))
    }
}

impl<T: DekuWriter<Ctx>, const N: usize, LenT: LenType, Ctx: Copy> DekuWriter<Ctx> for Vec<T, N, LenT> {
    /// Write all `T`s in a `Vec` to bits.
    /// * **inner_ctx** - The context required by `T`.
    /// # Examples
    /// ```rust
    /// # use deku::{ctx::Endian, DekuWriter};
    /// # use deku::writer::Writer;
    /// # #[cfg(feature = "bits")]
    /// # use deku::bitvec::{Msb0, bitvec};
    /// # #[cfg(feature = "std")]
    /// # use std::io::Cursor;
    ///
    /// # #[cfg(feature = "std")]
    /// # fn main() {
    /// let data = vec![1u8];
    /// let mut out_buf = vec![];
    /// let mut cursor = Cursor::new(&mut out_buf);
    /// let mut writer = Writer::new(&mut cursor);
    /// data.to_writer(&mut writer, Endian::Big).unwrap();
    /// assert_eq!(data, out_buf.to_vec());
    /// # }
    ///
    /// # #[cfg(not(feature = "std"))]
    /// # fn main() {}
    /// ```
    fn to_writer<W: Write + Seek>(
        &self,
        writer: &mut Writer<W>,
        inner_ctx: Ctx,
    ) -> Result<(), DekuError> {
        for v in self {
            v.to_writer(writer, inner_ctx)?;
        }
        Ok(())
    }
}

