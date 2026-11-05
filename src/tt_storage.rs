//! Owned TT storage: Linux transparent huge-page advice, Vec elsewhere.
//! Algorithms remain safe Rust. Only native allocation/advice/slice ownership uses FFI.
//! Allocation policy follows 7-Zip C/Alloc.c at revision 9128b80e3a471108678e2b3ec3c985861c8d7b0f.
//! SPDX-License-Identifier: LGPL-2.1-or-later
#![deny(unsafe_code)]

use std::ops::{Deref, DerefMut};

mod sealed {
    pub trait Sealed {}
    impl Sealed for u32 {}
    impl Sealed for [u32; 2] {}
}

/// Only the two fixed, all-zero-valid TT representations may own native storage.
pub(crate) trait Word: sealed::Sealed + Copy + Default + Send + Sync {
    const MAX_ALLOCATION: usize;
}
impl Word for u32 {
    const MAX_ALLOCATION: usize = 4 << 20;
}
impl Word for [u32; 2] {
    const MAX_ALLOCATION: usize = 8 << 20;
}

const MAX_WORDS: usize = 900_000;
const HUGE_PAGE: usize = 2 << 20;
const THRESHOLD: usize = 1 << 20;

fn allocation_bytes<W: Word>(words: usize) -> Option<usize> {
    if words > MAX_WORDS {
        return None;
    }
    let bytes = words.checked_mul(std::mem::size_of::<W>())?;
    if bytes <= THRESHOLD {
        return None;
    }
    bytes
        .checked_add(HUGE_PAGE - 1)
        .map(|size| size & !(HUGE_PAGE - 1))
        .filter(|&size| size <= W::MAX_ALLOCATION)
}

pub(crate) enum TtBuffer<W: Word = u32> {
    Portable(Vec<W>),
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64"),
        not(miri)
    ))]
    Native(native::Buffer<W>),
}

impl<W: Word> Default for TtBuffer<W> {
    fn default() -> Self {
        Self::Portable(Vec::new())
    }
}

impl<W: Word> From<Vec<W>> for TtBuffer<W> {
    fn from(values: Vec<W>) -> Self {
        Self::Portable(values)
    }
}

impl<W: Word> TtBuffer<W> {
    fn capacity(&self) -> usize {
        match self {
            Self::Portable(values) => values.capacity(),
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            ))]
            Self::Native(values) => values.capacity(),
        }
    }

    /// Sets the initialized logical extent, retaining possibly dirty words on reuse.
    /// This is not a zero-on-regrow API: TT scatter replaces every word before reads.
    pub(crate) fn resized(mut self, length: usize) -> Self {
        assert!(length <= MAX_WORDS, "TT exceeds the format bound");
        if self.capacity() < length {
            // Do not hold an old tree while allocating its replacement.
            drop(self);
            self = Self::allocate(length);
        }
        match &mut self {
            Self::Portable(values) => values.resize(length, W::default()),
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            ))]
            Self::Native(values) => values.set_len(length),
        }
        self
    }

    fn allocate(length: usize) -> Self {
        Self::allocate_with(length, |bytes| {
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            ))]
            {
                native::Buffer::new(bytes).map(Self::Native)
            }
            #[cfg(not(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            )))]
            {
                let _ = bytes;
                None
            }
        })
    }

    fn allocate_with(length: usize, attempt: impl FnOnce(usize) -> Option<Self>) -> Self {
        if let Some(bytes) = allocation_bytes::<W>(length)
            && let Some(storage) = attempt(bytes)
        {
            return storage;
        }
        Self::Portable(Vec::with_capacity(length))
    }
}

impl<W: Word> Deref for TtBuffer<W> {
    type Target = [W];
    fn deref(&self) -> &[W] {
        match self {
            Self::Portable(values) => values,
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            ))]
            Self::Native(values) => values.as_slice(),
        }
    }
}

impl<W: Word> DerefMut for TtBuffer<W> {
    fn deref_mut(&mut self) -> &mut [W] {
        match self {
            Self::Portable(values) => values,
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            ))]
            Self::Native(values) => values.as_mut_slice(),
        }
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    not(miri)
))]
#[allow(unsafe_code)]
mod native {
    use super::Word;
    use std::ffi::{c_int, c_void};
    use std::ptr::NonNull;
    const MADV_HUGEPAGE: c_int = 14;
    // SAFETY: these are the Linux C allocator/advice ABIs on the two cfg-selected targets.
    unsafe extern "C" {
        fn posix_memalign(pointer: *mut *mut c_void, alignment: usize, size: usize) -> c_int;
        fn madvise(pointer: *mut c_void, length: usize, advice: c_int) -> c_int;
        fn free(pointer: *mut c_void);
    }
    pub(crate) struct Buffer<W: Word> {
        pointer: NonNull<W>,
        bytes: usize,
        length: usize,
    }
    // SAFETY: Buffer uniquely owns its C allocation; moving it never creates aliases.
    // Word is sealed to the two Send integer representations, with no borrowed data.
    unsafe impl<W: Word> Send for Buffer<W> {}
    // SAFETY: Word is sealed to Sync integers with no interior mutability. Shared
    // access exposes only immutable slices; mutation and length changes require
    // &mut self, and deallocation requires ownership. Shared access exposes no pointer.
    unsafe impl<W: Word> Sync for Buffer<W> {}
    impl<W: Word> Buffer<W> {
        pub(super) fn new(bytes: usize) -> Option<Self> {
            Self::allocate(bytes, |pointer, length| {
                // SAFETY: allocate supplies the complete owned, aligned range.
                unsafe { madvise(pointer, length, MADV_HUGEPAGE) }
            })
        }
        fn allocate(
            bytes: usize,
            advice: impl FnOnce(*mut c_void, usize) -> c_int,
        ) -> Option<Self> {
            if bytes == 0 || bytes > W::MAX_ALLOCATION || !bytes.is_multiple_of(super::HUGE_PAGE) {
                return None;
            }
            let mut pointer = std::ptr::null_mut();
            // SAFETY: pointer is writable, alignment is a power of two and a pointer multiple.
            let status = unsafe { posix_memalign(&mut pointer, super::HUGE_PAGE, bytes) };
            if status != 0 {
                return None;
            }
            let pointer = NonNull::new(pointer.cast::<W>())?;
            // Own storage before invoking advice, so even a test callback panic frees it.
            let buffer = Self {
                pointer,
                bytes,
                length: 0,
            };
            // Advice failure does not invalidate a successful allocation.
            let _ = advice(pointer.as_ptr().cast(), bytes);
            // SAFETY: posix_memalign returned bytes writable bytes, uniquely owned by buffer.
            // Word is sealed to u32 and [u32; 2]: both accept all-zero bits, align to four,
            // and their four/eight-byte sizes divide the checked huge-page byte extent.
            // Initializing the complete range also faults in the advised huge pages.
            unsafe {
                pointer
                    .as_ptr()
                    .write_bytes(0, bytes / std::mem::size_of::<W>())
            };
            Some(buffer)
        }
        pub(super) fn capacity(&self) -> usize {
            self.bytes / std::mem::size_of::<W>()
        }
        pub(super) fn set_len(&mut self, length: usize) {
            assert!(length <= self.capacity());
            self.length = length;
        }
        pub(super) fn as_slice(&self) -> &[W] {
            // SAFETY: the whole allocation was initialized; length is checked against capacity.
            // The borrow cannot outlive this owner and only exposes logical TT words.
            unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
        }
        pub(super) fn as_mut_slice(&mut self) -> &mut [W] {
            // SAFETY: &mut self proves exclusive access to initialized, bounded logical words.
            unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.length) }
        }
    }
    impl<W: Word> Drop for Buffer<W> {
        fn drop(&mut self) {
            // SAFETY: this owner alone holds the allocation, created by posix_memalign.
            // No borrowed slice can outlive &self, and C free matches its allocator.
            unsafe { free(self.pointer.as_ptr().cast()) };
        }
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn shared_native_words_are_read_only_until_all_borrows_end() {
            let mut compact = super::Buffer::<u32>::new(2 << 20).unwrap();
            compact.set_len(257);
            compact.as_mut_slice().fill(17);
            let mut paired = super::Buffer::<[u32; 2]>::new(2 << 20).unwrap();
            paired.set_len(257);
            paired.as_mut_slice().fill([19, 31]);
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    let compact = &compact;
                    let paired = &paired;
                    scope.spawn(move || {
                        assert_eq!(compact.as_slice(), [17; 257]);
                        assert_eq!(paired.as_slice(), [[19, 31]; 257]);
                    });
                }
            });
            // Exclusive access and eventual destruction resume after every join.
            compact.as_mut_slice()[0] = 47;
            paired.as_mut_slice()[0] = [53, 59];
            assert_eq!(compact.as_slice()[0], 47);
            assert_eq!(paired.as_slice()[0], [53, 59]);
        }
        #[test]
        fn advice_is_inside_the_owned_alignment_and_failure_keeps_zeroed_storage() {
            let mut buffer = super::Buffer::<u32>::allocate(4 << 20, |pointer, bytes| {
                assert_eq!(pointer as usize % super::super::HUGE_PAGE, 0);
                assert_eq!(bytes, 4 << 20);
                -1
            })
            .unwrap();
            buffer.set_len(900_000);
            assert!(buffer.as_slice().iter().all(|&word| word == 0));
            buffer.as_mut_slice()[899_999] = 17;
            assert_eq!(buffer.as_slice()[899_999], 17);
        }
        #[test]
        fn paired_native_storage_checks_its_own_ceiling_zeroes_and_ownership() {
            for bytes in [0, 1, (2 << 20) + 1, 10 << 20, usize::MAX] {
                assert!(
                    super::Buffer::<[u32; 2]>::allocate(bytes, |_, _| panic!(
                        "invalid paired range advised"
                    ))
                    .is_none()
                );
            }
            assert!(
                super::Buffer::<u32>::allocate(6 << 20, |_, _| panic!("compact ceiling exceeded"))
                    .is_none()
            );
            drop(super::Buffer::<[u32; 2]>::new(6 << 20).unwrap());
            let mut buffer = super::Buffer::<[u32; 2]>::allocate(8 << 20, |pointer, bytes| {
                assert_eq!(pointer as usize % super::super::HUGE_PAGE, 0);
                assert_eq!(bytes, 8 << 20);
                -1
            })
            .unwrap();
            buffer.set_len(900_000);
            assert!(buffer.as_slice().iter().all(|&word| word == [0; 2]));
            buffer.as_mut_slice()[899_999] = [17, 31];
            buffer = std::thread::spawn(move || {
                assert_eq!(buffer.as_slice()[899_999], [17, 31]);
                buffer
            })
            .join()
            .unwrap();
            let too_large = buffer.capacity() + 1;
            assert!(
                std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| buffer.set_len(too_large))
                )
                .is_err()
            );
            assert_eq!(buffer.as_slice()[899_999], [17, 31]);
            drop(buffer);
            assert!(
                std::panic::catch_unwind(|| super::Buffer::<[u32; 2]>::allocate(
                    8 << 20,
                    |_, _| panic!("paired advice panic")
                ))
                .is_err()
            );
        }
        #[test]
        fn native_extent_rejects_invalid_sizes_and_hides_spare_words() {
            for bytes in [0, 1, (2 << 20) + 1, 6 << 20, usize::MAX] {
                assert!(
                    super::Buffer::<u32>::allocate(bytes, |_, _| panic!("invalid range advised"))
                        .is_none()
                );
            }
            let mut buffer = super::Buffer::<u32>::new(2 << 20).unwrap();
            buffer.set_len(3);
            assert_eq!(buffer.as_slice().len(), 3);
            assert!(buffer.as_slice().get(3).is_none());
            buffer.as_mut_slice().copy_from_slice(&[17, 31, 47]);
            let too_large = buffer.capacity() + 1;
            assert!(
                std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| buffer.set_len(too_large))
                )
                .is_err()
            );
            assert_eq!(buffer.as_slice(), [17, 31, 47]);
        }
        #[test]
        fn advice_panic_releases_owned_storage_during_unwind() {
            // Memcheck/ASan exercise this real FFI owner: initialization has
            // not happened yet, but Drop must still free the C allocation.
            assert!(
                std::panic::catch_unwind(|| super::Buffer::<u32>::allocate(
                    2 << 20,
                    |_, _| panic!("injected advice panic")
                ))
                .is_err()
            );
            let mut live = super::Buffer::<u32>::new(2 << 20).unwrap();
            live.set_len(1);
            assert_eq!(live.as_slice(), [0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_rounding_and_format_bound_are_checked() {
        for words in [0, 1, THRESHOLD / 4] {
            assert_eq!(allocation_bytes::<u32>(words), None);
        }
        assert_eq!(allocation_bytes::<u32>(THRESHOLD / 4 + 1), Some(HUGE_PAGE));
        assert_eq!(
            allocation_bytes::<u32>(HUGE_PAGE / 4 + 1),
            Some(2 * HUGE_PAGE)
        );
        assert_eq!(allocation_bytes::<u32>(900_000), Some(4 << 20));
        assert_eq!(allocation_bytes::<u32>(900_001), None);
        assert_eq!(allocation_bytes::<u32>(usize::MAX), None);
    }
    #[test]
    fn storage_reuses_dirty_capacity_and_moves_between_threads() {
        let mut buffer = TtBuffer::<u32>::default();
        let lengths: &[usize] = if cfg!(miri) {
            &[1, 257, 4097, 65, 5000]
        } else {
            &[1, 257, 300_000, 900_000, 65, 500_000]
        };
        for &length in lengths {
            buffer = buffer.resized(length);
            assert_eq!(buffer.len(), length);
            buffer.fill(0xABCD_EF01);
            buffer[length - 1] = length as u32;
            buffer = std::thread::spawn(move || {
                assert_eq!(buffer[length - 1], length as u32);
                buffer
            })
            .join()
            .unwrap();
        }
        drop(buffer);
    }
    #[test]
    fn independent_owners_and_portable_fallback_keep_live_words() {
        let mut first = TtBuffer::<u32>::from(vec![31; 1024]).resized(100);
        let second = TtBuffer::<u32>::from(vec![17; 2048]).resized(1500);
        first[0] = 19;
        drop(second);
        assert_eq!(first[0], 19);
        assert!(first[1..].iter().all(|&word| word == 31));
    }
    #[test]
    fn aligned_allocation_refusal_uses_a_normal_initialized_buffer() {
        let logical = if cfg!(miri) { 33 } else { 300_000 };
        let buffer = TtBuffer::<u32>::allocate_with(300_000, |bytes| {
            assert_eq!(bytes, HUGE_PAGE);
            None
        })
        .resized(logical);
        assert!(matches!(&buffer, TtBuffer::<u32>::Portable(_)));
        assert_eq!(buffer.len(), logical);
        assert!(buffer.iter().all(|&word| word == 0));
        assert!(
            std::panic::catch_unwind(|| TtBuffer::<u32>::default().resized(MAX_WORDS + 1)).is_err()
        );
    }
}

#[cfg(test)]
mod word_tests;
