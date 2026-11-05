use super::{Decoder, Mode};

#[test]
fn factories_select_an_immutable_platform_or_compact_mode_before_allocation() {
    let platform = Decoder::with_threads(std::io::empty(), 8);
    assert_eq!(
        platform.threads,
        if cfg!(target_family = "wasm") { 1 } else { 2 }
    );
    assert_eq!(
        matches!(platform.mode, Mode::Paired),
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
    );
    let compact = Decoder::with_low_memory(std::io::empty(), 8);
    assert!(matches!(compact.mode, Mode::Compact));
    let callback = Decoder::with_low_memory_output_callback(std::io::empty(), 8, |_| {});
    assert!(matches!(callback.mode, Mode::Compact));
    assert!(callback.callback.is_some());
    assert!(platform.active.is_none());
    assert!(compact.active.is_none());
    assert!(callback.active.is_none());
}
