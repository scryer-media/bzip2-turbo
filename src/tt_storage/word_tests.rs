use super::{HUGE_PAGE, MAX_WORDS, THRESHOLD, TtBuffer, Word, allocation_bytes};
use std::fmt::Debug;

fn geometry<W: Word>() {
    let width = std::mem::size_of::<W>();
    for length in [0, 1, THRESHOLD / width] {
        assert_eq!(allocation_bytes::<W>(length), None);
    }
    assert_eq!(
        allocation_bytes::<W>(THRESHOLD / width + 1),
        Some(HUGE_PAGE)
    );
    assert_eq!(allocation_bytes::<W>(MAX_WORDS), Some(W::MAX_ALLOCATION));
    for length in [MAX_WORDS + 1, usize::MAX] {
        assert_eq!(allocation_bytes::<W>(length), None);
    }
}

#[test]
fn both_word_widths_obey_logical_and_rounded_byte_bounds() {
    geometry::<u32>();
    geometry::<[u32; 2]>();
}

fn ownership<W: Word + PartialEq + Debug + 'static>(value: impl Fn(usize) -> W) {
    let lengths: &[usize] = if cfg!(miri) {
        &[1, 257, 4097, 65, 5000]
    } else {
        &[1, 257, 300_000, MAX_WORDS, 65, 500_000]
    };
    let mut buffer = TtBuffer::<W>::default();
    for &length in lengths {
        buffer = buffer.resized(length);
        assert_eq!(buffer.len(), length);
        for (index, word) in buffer.iter_mut().enumerate() {
            *word = value(index);
        }
        let expected = value(length - 1);
        buffer = std::thread::spawn(move || {
            assert_eq!(buffer[length - 1], expected);
            buffer
        })
        .join()
        .unwrap();
        let retained = buffer[0];
        buffer = buffer.resized(1).resized(length);
        assert_eq!(buffer[0], retained);
    }
    let retained = buffer[0];
    drop(TtBuffer::<W>::default().resized(17));
    assert_eq!(buffer[0], retained);
}

#[test]
fn both_word_widths_preserve_dirty_prefix_and_independent_threaded_owners() {
    ownership::<u32>(|index| 0xABCD_EF01 ^ index as u32);
    ownership::<[u32; 2]>(|index| [index as u32, !(index as u32)]);
}

fn refusal<W: Word + PartialEq + Debug>() {
    let requested = THRESHOLD / std::mem::size_of::<W>() + 1;
    let logical = if cfg!(miri) { 33 } else { requested };
    let buffer = TtBuffer::<W>::allocate_with(requested, |bytes| {
        assert_eq!(bytes, HUGE_PAGE);
        None
    })
    .resized(logical);
    assert!(matches!(&buffer, TtBuffer::Portable(_)));
    assert_eq!(buffer.len(), logical);
    assert!(buffer.iter().all(|word| *word == W::default()));
    assert!(std::panic::catch_unwind(|| TtBuffer::<W>::default().resized(MAX_WORDS + 1)).is_err());
}

#[test]
fn both_word_widths_fall_back_to_initialized_storage_after_refusal() {
    refusal::<u32>();
    refusal::<[u32; 2]>();
}
