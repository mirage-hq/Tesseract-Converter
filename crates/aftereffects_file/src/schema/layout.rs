//! Small fixed-layout primitives for byte-preserving typed records.

use super::RecordError;

/// One authoritative fixed-size encoded record image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RecordImage<const N: usize>([u8; N]);

impl<const N: usize> RecordImage<N> {
    pub(super) fn zeroed() -> Self {
        Self([0; N])
    }

    pub(super) fn decode(bytes: &[u8], record: &'static str) -> Result<Self, RecordError> {
        bytes.try_into().map(Self).map_err(|_| RecordError::Length {
            record,
            expected: N,
            actual: bytes.len(),
        })
    }

    pub(super) fn bytes(&self) -> &[u8; N] {
        &self.0
    }

    pub(super) fn bytes_mut(&mut self) -> &mut [u8; N] {
        &mut self.0
    }

    pub(super) fn encode(&self) -> [u8; N] {
        self.0
    }
}

#[derive(Clone, Copy)]
pub(super) struct U16Field<const N: usize> {
    offset: usize,
}

impl<const N: usize> U16Field<N> {
    pub(super) const fn new(offset: usize) -> Self {
        assert!(N >= size_of::<u16>() && offset <= N - size_of::<u16>());
        Self { offset }
    }

    pub(super) fn get(self, image: &RecordImage<N>) -> u16 {
        u16::from_be_bytes(
            image.bytes()[self.offset..self.offset + size_of::<u16>()]
                .try_into()
                .expect("field bounds checked at declaration"),
        )
    }

    pub(super) fn set(self, image: &mut RecordImage<N>, value: u16) {
        image.bytes_mut()[self.offset..self.offset + size_of::<u16>()]
            .copy_from_slice(&value.to_be_bytes());
    }
}

#[derive(Clone, Copy)]
pub(super) struct U32Field<const N: usize> {
    offset: usize,
}

impl<const N: usize> U32Field<N> {
    pub(super) const fn new(offset: usize) -> Self {
        assert!(N >= size_of::<u32>() && offset <= N - size_of::<u32>());
        Self { offset }
    }

    pub(super) fn get(self, image: &RecordImage<N>) -> u32 {
        u32::from_be_bytes(
            image.bytes()[self.offset..self.offset + size_of::<u32>()]
                .try_into()
                .expect("field bounds checked at declaration"),
        )
    }

    pub(super) fn set(self, image: &mut RecordImage<N>, value: u32) {
        image.bytes_mut()[self.offset..self.offset + size_of::<u32>()]
            .copy_from_slice(&value.to_be_bytes());
    }
}

#[derive(Clone, Copy)]
pub(super) struct I32Field<const N: usize> {
    offset: usize,
}

impl<const N: usize> I32Field<N> {
    pub(super) const fn new(offset: usize) -> Self {
        assert!(N >= size_of::<i32>() && offset <= N - size_of::<i32>());
        Self { offset }
    }

    pub(super) fn get(self, image: &RecordImage<N>) -> i32 {
        i32::from_be_bytes(
            image.bytes()[self.offset..self.offset + size_of::<i32>()]
                .try_into()
                .expect("field bounds checked at declaration"),
        )
    }

    pub(super) fn set(self, image: &mut RecordImage<N>, value: i32) {
        image.bytes_mut()[self.offset..self.offset + size_of::<i32>()]
            .copy_from_slice(&value.to_be_bytes());
    }
}
