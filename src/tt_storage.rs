//! Owned TT and last-column storage: Linux transparent huge-page advice, Vec elsewhere.
//! Algorithms remain safe Rust. Only native mapping/advice/slice ownership uses FFI.
//! Native buffers are anonymous `mmap` extents released with `munmap`, so they are
//! independent of the process allocator (Rust's global allocator and the C `malloc`).
//! Allocation policy follows 7-Zip C/Alloc.c at revision 9128b80e3a471108678e2b3ec3c985861c8d7b0f.
//! SPDX-License-Identifier: LGPL-2.1-or-later
#![deny(unsafe_code)]

use std::ops::{Deref, DerefMut};

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u32 {}
    impl Sealed for [u32; 2] {}
}

/// Only the fixed, all-zero-valid last-column and TT representations may own
/// native storage. Each ceiling is the format bound of 900,000 elements rounded
/// up to whole 2 MiB pages.
pub(crate) trait Word: sealed::Sealed + Copy + Default + Send + Sync {
    const MAX_ALLOCATION: usize;
}
impl Word for u8 {
    const MAX_ALLOCATION: usize = 2 << 20;
}
impl Word for u32 {
    const MAX_ALLOCATION: usize = 4 << 20;
}
impl Word for [u32; 2] {
    const MAX_ALLOCATION: usize = 8 << 20;
}

/// Whether this target backs every TT and last-column buffer with advised
/// 2 MiB pages, so allocation bounds must count the rounded extents.
pub(crate) const NATIVE_HUGE_PAGES: bool = cfg!(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    not(miri)
));

const MAX_WORDS: usize = 900_000;
const HUGE_PAGE: usize = 2 << 20;

// BigAlloc takes its aligned, advised path only for sizes above
// g_LargePageThresholdMin (1 MiB). It aligns those to 2 MiB, and rounds the size
// up to a 2 MiB multiple only when the remainder past whole multiples exceeds
// 1 MiB; otherwise it keeps the size as is. CDecoder passes one allocation of more
// than 1 MiB, so every block is advised. Here each separately owned buffer is
// always rounded up to whole 2 MiB pages, at least one: a deliberate superset of
// that policy, so every TT and column is aligned, advised and fully huge-page sized.
fn allocation_bytes<W: Word>(words: usize) -> Option<usize> {
    if words > MAX_WORDS {
        return None;
    }
    let bytes = words.checked_mul(std::mem::size_of::<W>())?.max(1);
    bytes
        .checked_add(HUGE_PAGE - 1)
        .map(|size| size & !(HUGE_PAGE - 1))
        .filter(|&size| size <= W::MAX_ALLOCATION)
}

/// Vec storage with a logical length below the initialized extent. `values.len()`
/// is the high-water mark of words ever initialized, so shrinking and regrowing
/// within it neither zero-fills nor exposes uninitialized words.
pub(crate) struct Portable<W> {
    values: Vec<W>,
    length: usize,
}

impl<W: Word> Portable<W> {
    fn with_capacity(words: usize) -> Self {
        Self {
            values: Vec::with_capacity(words),
            length: 0,
        }
    }
    fn set_len(&mut self, length: usize) {
        if length > self.values.len() {
            // Only words past the high-water mark are new; they start zeroed.
            self.values.resize(length, W::default());
        }
        self.length = length;
    }
}

pub(crate) enum TtBuffer<W: Word = u32> {
    Portable(Portable<W>),
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64"),
        not(miri)
    ))]
    Native(native::Buffer<W>),
}

impl<W: Word> Default for TtBuffer<W> {
    fn default() -> Self {
        Self::Portable(Portable::with_capacity(0))
    }
}

impl<W: Word> From<Vec<W>> for TtBuffer<W> {
    fn from(values: Vec<W>) -> Self {
        Self::Portable(Portable {
            length: values.len(),
            values,
        })
    }
}

impl<W: Word> TtBuffer<W> {
    fn capacity(&self) -> usize {
        match self {
            Self::Portable(values) => values.values.capacity(),
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                not(miri)
            ))]
            Self::Native(values) => values.capacity(),
        }
    }

    /// Ensures capacity for `words` without ever shrinking or reallocating a
    /// buffer that already holds them. A too-small buffer is released before its
    /// replacement is allocated, so its words are discarded (logical length zero).
    /// CDecoder sizes its one `_counters` allocation at the format maximum; the
    /// decoder sizes each owner at the stream's declared block limit instead.
    pub(crate) fn with_capacity_at_least(self, words: usize) -> Self {
        assert!(words <= MAX_WORDS, "TT exceeds the format bound");
        if self.capacity() >= words {
            return self;
        }
        drop(self);
        Self::allocate(words)
    }

    /// Sets the initialized logical extent, retaining possibly dirty words on reuse.
    /// This is not a zero-on-regrow API: TT scatter replaces every word before reads.
    /// Words beyond any length this owner has held start zeroed (the kernel's zero
    /// pages natively, a one-time fill on the portable path); words within it keep
    /// whatever they last held, so a shrink never forces a later zero fill.
    pub(crate) fn resized(mut self, length: usize) -> Self {
        assert!(length <= MAX_WORDS, "TT exceeds the format bound");
        if self.capacity() < length {
            // Do not hold an old tree while allocating its replacement.
            drop(self);
            self = Self::allocate(length);
        }
        match &mut self {
            Self::Portable(values) => values.set_len(length),
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
        Self::Portable(Portable::with_capacity(length))
    }
}

impl<W: Word> Deref for TtBuffer<W> {
    type Target = [W];
    fn deref(&self) -> &[W] {
        match self {
            Self::Portable(values) => &values.values[..values.length],
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
            Self::Portable(values) => &mut values.values[..values.length],
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
    //! Anonymous private mappings, so no allocator is involved: neither Rust's
    //! `#[global_allocator]` nor whichever C `malloc` the process links (glibc,
    //! mimalloc, jemalloc, ...). Each buffer is mapped with 2 MiB of slack, the
    //! slack outside the 2 MiB-aligned extent is unmapped, and `Drop` unmaps
    //! exactly the extent, so placement and release do not depend on any
    //! allocator's thresholds, caching or alignment policy.
    use super::{HUGE_PAGE, Word};
    use std::ffi::{c_int, c_void};
    use std::ptr::NonNull;
    const PROT_READ: c_int = 1;
    const PROT_WRITE: c_int = 2;
    const MAP_PRIVATE: c_int = 2;
    const MAP_ANONYMOUS: c_int = 0x20;
    const MADV_HUGEPAGE: c_int = 14;
    const MAP_FAILED: *mut c_void = std::ptr::without_provenance_mut(usize::MAX);
    // SAFETY: these are the Linux mmap/munmap/madvise ABIs on the two cfg-selected
    // targets, where off_t is 64 bits and the constants above have these values.
    unsafe extern "C" {
        fn mmap(
            address: *mut c_void,
            length: usize,
            protection: c_int,
            flags: c_int,
            descriptor: c_int,
            offset: i64,
        ) -> *mut c_void;
        fn munmap(address: *mut c_void, length: usize) -> c_int;
        fn madvise(pointer: *mut c_void, length: usize, advice: c_int) -> c_int;
    }

    /// Bytes to unmap before and after the first 2 MiB boundary at or above a
    /// mapping that starts at `address` and spans an extent plus `HUGE_PAGE`.
    fn slack(address: usize) -> (usize, usize) {
        let leading = address.wrapping_neg() & (HUGE_PAGE - 1);
        (leading, HUGE_PAGE - leading)
    }

    /// Maps `length` bytes of fresh, zeroed, private read/write memory.
    fn map_anonymous(length: usize) -> *mut c_void {
        // SAFETY: a null hint lets the kernel choose a fresh range, so no existing
        // mapping is replaced; an anonymous mapping ignores descriptor and offset.
        unsafe {
            mmap(
                std::ptr::null_mut(),
                length,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        }
    }

    pub(crate) struct Buffer<W: Word> {
        pointer: NonNull<W>,
        bytes: usize,
        length: usize,
    }
    // SAFETY: Buffer uniquely owns its private anonymous mapping; moving it never
    // creates aliases. Word is sealed to the three Send integer representations,
    // with no borrowed data.
    unsafe impl<W: Word> Send for Buffer<W> {}
    // SAFETY: Word is sealed to Sync integers with no interior mutability. Shared
    // access exposes only immutable slices; mutation and length changes require
    // &mut self, and unmapping requires ownership. Shared access exposes no pointer.
    unsafe impl<W: Word> Sync for Buffer<W> {}
    impl<W: Word> Buffer<W> {
        pub(super) fn new(bytes: usize) -> Option<Self> {
            Self::allocate(bytes, |pointer, length| {
                // SAFETY: allocate supplies the complete owned, aligned mapped range.
                unsafe { madvise(pointer, length, MADV_HUGEPAGE) }
            })
        }
        fn allocate(
            bytes: usize,
            advice: impl FnOnce(*mut c_void, usize) -> c_int,
        ) -> Option<Self> {
            Self::allocate_mapped(bytes, map_anonymous, advice)
        }
        /// `map` stands in for the anonymous `mmap`: it returns either
        /// `MAP_FAILED` or a fresh, uniquely owned, page-aligned, zero-initialized
        /// read/write mapping of exactly the requested length.
        fn allocate_mapped(
            bytes: usize,
            map: impl FnOnce(usize) -> *mut c_void,
            advice: impl FnOnce(*mut c_void, usize) -> c_int,
        ) -> Option<Self> {
            if bytes == 0 || bytes > W::MAX_ALLOCATION || !bytes.is_multiple_of(HUGE_PAGE) {
                return None;
            }
            // Cannot overflow: bytes is at most W::MAX_ALLOCATION (8 MiB).
            let mapped = map(bytes + HUGE_PAGE);
            if mapped == MAP_FAILED || mapped.is_null() {
                return None;
            }
            let (leading, trailing) = slack(mapped as usize);
            if leading != 0 {
                // SAFETY: [mapped, mapped + leading) is the owned mapping's head below
                // the 2 MiB boundary. mmap returns page-aligned addresses and both 2 MiB
                // and the boundary are page multiples, so leading is a page multiple.
                // Failure (only EINVAL/ENOMEM) leaves the head mapped but unused.
                let _ = unsafe { munmap(mapped, leading) };
            }
            // leading < HUGE_PAGE, so the aligned start lies inside the mapping.
            let aligned = mapped.wrapping_byte_add(leading);
            // trailing is never zero: leading < HUGE_PAGE.
            // SAFETY: [aligned + bytes, aligned + bytes + trailing) ends exactly at the
            // owned mapping's end (leading + bytes + trailing == bytes + HUGE_PAGE) and
            // starts on a page boundary; nothing else references it.
            let _ = unsafe { munmap(aligned.wrapping_byte_add(bytes), trailing) };
            let pointer = NonNull::new(aligned.cast::<W>())?;
            // Own the extent before invoking advice, so even a test callback panic unmaps it.
            let buffer = Self {
                pointer,
                bytes,
                length: 0,
            };
            // Advice failure does not invalidate a successful mapping. It precedes any
            // touch, so with THP enabled first-touch faults allocate 2 MiB pages. As in
            // BigAlloc, nothing is pre-filled: pages fault in lazily, already zeroed.
            let _ = advice(aligned, bytes);
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
            // SAFETY: [pointer, pointer + bytes) is a uniquely owned private anonymous
            // read/write mapping, which the kernel zero-initializes (on first touch), so
            // every byte is initialized. Word is sealed to u8, u32 and [u32; 2]: all
            // accept all-zero bits and need at most four-byte alignment, which the 2 MiB
            // alignment covers. length is checked against capacity (bytes / size_of::<W>()).
            // The borrow cannot outlive this owner and only exposes logical TT words.
            unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
        }
        pub(super) fn as_mut_slice(&mut self) -> &mut [W] {
            // SAFETY: as in as_slice, the bounded logical words lie in the owned,
            // kernel-zeroed read/write mapping; &mut self proves exclusive access.
            unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.length) }
        }
    }
    impl<W: Word> Drop for Buffer<W> {
        fn drop(&mut self) {
            // SAFETY: this owner alone holds the mapped extent [pointer, pointer + bytes),
            // left mapped by allocate_mapped after its slack was unmapped. No borrowed
            // slice can outlive &self. Failure (only EINVAL) cannot occur for that range.
            let _ = unsafe { munmap(self.pointer.as_ptr().cast(), self.bytes) };
        }
    }
    #[cfg(test)]
    mod tests {
        use super::super::{HUGE_PAGE, TtBuffer, Word};
        #[test]
        fn slack_trims_to_exactly_one_aligned_extent() {
            for (address, leading) in [
                (0x4000_0000, 0),
                (0x4000_1000, HUGE_PAGE - 0x1000),
                (0x4010_0000, 1 << 20),
                (0x401f_f000, 0x1000),
                (0x4001_0000, HUGE_PAGE - 0x1_0000),
            ] {
                let (head, tail) = super::slack(address);
                assert_eq!(head, leading, "{address:#x}");
                assert_eq!((address + head) % HUGE_PAGE, 0);
                assert_eq!(head + tail, HUGE_PAGE);
                assert_ne!(tail, 0);
            }
        }
        #[test]
        fn failed_mapping_falls_back_to_a_portable_vec() {
            fn refused<W: Word + PartialEq>() {
                let mut requested = 0;
                assert!(
                    super::Buffer::<W>::allocate_mapped(
                        2 << 20,
                        |length| {
                            requested = length;
                            super::MAP_FAILED
                        },
                        |_, _| panic!("unmapped range advised")
                    )
                    .is_none()
                );
                assert_eq!(requested, (2 << 20) + HUGE_PAGE);
                let buffer = TtBuffer::<W>::allocate_with(17, |bytes| {
                    super::Buffer::allocate_mapped(bytes, |_| super::MAP_FAILED, |_, _| 0)
                        .map(TtBuffer::Native)
                })
                .resized(17);
                assert!(matches!(&buffer, TtBuffer::Portable(_)));
                assert!(buffer.iter().all(|&word| word == W::default()));
            }
            refused::<u8>();
            refused::<u32>();
            refused::<[u32; 2]>();
        }
        #[test]
        fn mapped_extent_is_huge_page_aligned_with_exact_capacity() {
            fn extent<W: Word>() {
                let mut bytes = HUGE_PAGE;
                while bytes <= W::MAX_ALLOCATION {
                    let buffer = super::Buffer::<W>::new(bytes).unwrap();
                    assert_eq!(buffer.pointer.as_ptr() as usize % HUGE_PAGE, 0);
                    assert_eq!(buffer.capacity(), bytes / std::mem::size_of::<W>());
                    bytes += HUGE_PAGE;
                }
            }
            extent::<u8>();
            extent::<u32>();
            extent::<[u32; 2]>();
        }
        #[test]
        fn dropped_extents_and_slack_are_returned_to_the_kernel() {
            fn mapped_kib() -> usize {
                let status = std::fs::read_to_string("/proc/self/status").unwrap();
                let line = status
                    .lines()
                    .find(|line| line.starts_with("VmSize:"))
                    .unwrap();
                line.split_whitespace().nth(1).unwrap().parse().unwrap()
            }
            // Without munmap of the extent or of its slack, 2048 rounds would
            // leave at least 4 GiB of address space mapped.
            let before = mapped_kib();
            for round in 0..2048 {
                let mut buffer = super::Buffer::<u8>::allocate(2 << 20, |_, _| 0).unwrap();
                buffer.set_len(1);
                buffer.as_mut_slice()[0] = round as u8;
                drop(buffer);
            }
            // Other test threads may map stacks or heap meanwhile; 256 MiB is far below a leak.
            assert!(mapped_kib() < before + (256 << 10));
        }
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
            // Memcheck/ASan exercise this real FFI owner: no page has been touched
            // yet, but Drop must still unmap the extent.
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
        #[test]
        fn byte_column_storage_is_one_zeroed_page_with_its_own_ceiling() {
            for bytes in [0, 1, (2 << 20) + 1, 4 << 20, usize::MAX] {
                assert!(
                    super::Buffer::<u8>::allocate(bytes, |_, _| panic!("invalid column advised"))
                        .is_none()
                );
            }
            let mut column = super::Buffer::<u8>::allocate(2 << 20, |pointer, bytes| {
                assert_eq!(pointer as usize % super::super::HUGE_PAGE, 0);
                assert_eq!(bytes, 2 << 20);
                -1
            })
            .unwrap();
            assert_eq!(column.capacity(), 2 << 20);
            column.set_len(900_000);
            assert!(column.as_slice().iter().all(|&byte| byte == 0));
            column.as_mut_slice()[899_999] = 0xa5;
            column = std::thread::spawn(move || {
                assert_eq!(column.as_slice()[899_999], 0xa5);
                column
            })
            .join()
            .unwrap();
            column.set_len(3);
            assert_eq!(column.as_slice(), [0; 3]);
        }
        #[test]
        fn every_buffer_size_is_huge_page_backed_and_kept_once_large_enough() {
            use super::super::TtBuffer;
            fn native<W: super::Word>(lengths: &[usize]) {
                let mut buffer = TtBuffer::<W>::default();
                for &length in lengths {
                    buffer = buffer.resized(length);
                    assert!(matches!(&buffer, TtBuffer::Native(_)), "length {length}");
                    assert_eq!(buffer.len(), length);
                }
                // Sizing at the declared limit never shrinks or reallocates.
                let address = buffer.as_ptr();
                buffer = buffer.with_capacity_at_least(1).resized(1).resized(17);
                assert_eq!(buffer.as_ptr(), address);
            }
            native::<u8>(&[1, 17, 100_000, 900_000]);
            native::<u32>(&[1, 17, 100_000, 262_144, 262_145, 900_000]);
            native::<[u32; 2]>(&[1, 17, 100_000, 262_145, 900_000]);
            assert_eq!(TtBuffer::<u32>::default().resized(1).capacity(), 1 << 19);
            assert_eq!(
                TtBuffer::<u32>::default()
                    .with_capacity_at_least(500_000)
                    .capacity(),
                1 << 19
            );
            assert_eq!(
                TtBuffer::<u32>::default()
                    .with_capacity_at_least(900_000)
                    .capacity(),
                1 << 20
            );
            assert_eq!(TtBuffer::<u8>::default().resized(1).capacity(), 2 << 20);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_rounding_and_format_bound_are_checked() {
        // No size threshold: every request rounds up to at least one page.
        for words in [0, 1, 100_000, 250_000, HUGE_PAGE / 4] {
            assert_eq!(allocation_bytes::<u32>(words), Some(HUGE_PAGE));
        }
        assert_eq!(
            allocation_bytes::<u32>(HUGE_PAGE / 4 + 1),
            Some(2 * HUGE_PAGE)
        );
        assert_eq!(allocation_bytes::<u32>(500_000), Some(HUGE_PAGE));
        assert_eq!(allocation_bytes::<u32>(600_000), Some(4 << 20));
        assert_eq!(allocation_bytes::<u32>(900_000), Some(4 << 20));
        assert_eq!(allocation_bytes::<u32>(900_001), None);
        assert_eq!(allocation_bytes::<u32>(usize::MAX), None);
        // The last column of any level fits one page.
        for bytes in [0, 1, 100_000, 900_000] {
            assert_eq!(allocation_bytes::<u8>(bytes), Some(HUGE_PAGE));
        }
        assert_eq!(allocation_bytes::<u8>(900_001), None);
        for (words, pages) in [
            (1, 1),
            (200_000, 1),
            (HUGE_PAGE / 8, 1),
            (HUGE_PAGE / 8 + 1, 2),
            (500_000, 2),
            (700_000, 3),
            (900_000, 4),
        ] {
            assert_eq!(allocation_bytes::<[u32; 2]>(words), Some(pages * HUGE_PAGE));
        }
    }
    #[test]
    fn declared_limit_capacity_is_never_shrunk_or_reallocated() {
        let small = if cfg!(miri) { 5 } else { 100_000 };
        let large = if cfg!(miri) { 33 } else { 900_000 };
        let mut buffer = TtBuffer::<u8>::default().with_capacity_at_least(small);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.capacity() >= small);
        let address = buffer.as_ptr();
        for length in [small, 1, small / 2, small] {
            buffer = buffer.with_capacity_at_least(small).resized(length);
            assert_eq!(buffer.len(), length);
            assert_eq!(buffer.as_ptr(), address);
        }
        buffer[0] = 0x5a;
        buffer = buffer.with_capacity_at_least(1);
        assert_eq!(buffer[0], 0x5a);
        buffer = buffer.with_capacity_at_least(large);
        assert!(buffer.capacity() >= large);
        assert!(std::panic::catch_unwind(|| buffer.with_capacity_at_least(MAX_WORDS + 1)).is_err());
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
    /// A Vec-backed owner on every target: refusing the native attempt keeps the
    /// test on the portable path even where huge-page storage exists.
    fn portable<W: Word>(capacity: usize) -> TtBuffer<W> {
        let buffer = TtBuffer::<W>::allocate_with(capacity, |_| None);
        assert!(matches!(&buffer, TtBuffer::Portable(_)));
        buffer
    }
    #[test]
    fn portable_shrink_keeps_dirty_words_and_regrowth_past_the_mark_is_zero() {
        let earlier = if cfg!(miri) { 65 } else { 100_000 };
        let mut buffer = portable::<u32>(2 * earlier).resized(earlier);
        for (index, word) in buffer.iter_mut().enumerate() {
            *word = index as u32 + 1;
        }
        buffer = buffer.resized(3);
        assert_eq!(&buffer[..], [1, 2, 3]);
        // Regrowing within the high-water mark retains the earlier dirty words.
        buffer = buffer.resized(earlier);
        assert!(matches!(&buffer, TtBuffer::Portable(_)));
        assert!(
            buffer
                .iter()
                .enumerate()
                .all(|(index, &word)| word == index as u32 + 1)
        );
        // Past the mark only fresh, zeroed words appear.
        buffer = buffer.resized(3).resized(2 * earlier);
        assert!(matches!(&buffer, TtBuffer::Portable(_)));
        assert_eq!(buffer[earlier - 1], earlier as u32);
        assert!(buffer[earlier..].iter().all(|&word| word == 0));
        assert_eq!(TtBuffer::from(vec![7_u8; 5]).resized(2).len(), 2);
    }
    #[test]
    fn portable_growth_beyond_the_mark_reads_only_initialized_zeroes() {
        let words = if cfg!(miri) { 33 } else { 300_000 };
        for length in [1, words / 2, words] {
            let buffer = portable::<[u32; 2]>(words).resized(length);
            assert_eq!(buffer.len(), length);
            assert!(buffer.iter().all(|&word| word == [0; 2]));
        }
        let column = portable::<u8>(words).resized(1).resized(words);
        assert!(column.iter().all(|&byte| byte == 0));
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
