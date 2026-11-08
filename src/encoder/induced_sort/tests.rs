use super::{MIN_INDUCED, Workspace, is_proper_power};
use crate::encoder::flag_sort;

struct Random(u32);

impl Random {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn bytes(&mut self, len: usize, alphabet: u32) -> Vec<u8> {
        (0..len).map(|_| (self.next() % alphabet) as u8).collect()
    }
}

// The smallest period by the prefix function: `input` is a proper power iff
// that period is shorter than the input and divides its length.
fn prefix_function_power(input: &[u8]) -> bool {
    let mut border = vec![0usize; input.len()];
    for i in 1..input.len() {
        let mut k = border[i - 1];
        while k > 0 && input[i] != input[k] {
            k = border[k - 1];
        }
        if input[i] == input[k] {
            k += 1;
        }
        border[i] = k;
    }
    let period = input.len() - border.last().copied().unwrap_or(0);
    period < input.len() && input.len().is_multiple_of(period)
}

// Checks the induced path itself, below the size threshold too, against the
// existing sorter: same rotation order, same origin.
fn check_induced(induced: &mut Workspace, fallback: &mut flag_sort::Workspace, input: &[u8]) {
    let label = format!(
        "len={} head={:?}",
        input.len(),
        &input[..input.len().min(12)]
    );
    assert!(!is_proper_power(input), "{label}");
    let (expected, expected_origin) = fallback.sort(input).unwrap();
    let (actual, origin) = induced.induced(input).unwrap();
    assert_eq!(origin, expected_origin, "origin {label}");
    assert!(actual == expected, "rotation order {label}");
}

// Checks the routed entry point, which every encoder block uses.
fn check_sort(induced: &mut Workspace, fallback: &mut flag_sort::Workspace, input: &[u8]) {
    let label = format!(
        "len={} head={:?}",
        input.len(),
        &input[..input.len().min(12)]
    );
    let (expected, expected_origin) = fallback.sort(input).unwrap();
    let (actual, origin) = induced.sort(input).unwrap();
    assert_eq!(origin, expected_origin, "origin {label}");
    assert!(actual == expected, "rotation order {label}");
}

#[test]
fn power_detection_matches_the_prefix_function() {
    for len in 1..=14usize {
        for alphabet in [2u32, 3] {
            if alphabet == 3 && len > 9 {
                continue;
            }
            let total = (alphabet as usize).pow(len as u32);
            for mut code in 0..total {
                let input: Vec<_> = (0..len)
                    .map(|_| {
                        let digit = (code % alphabet as usize) as u8;
                        code /= alphabet as usize;
                        digit
                    })
                    .collect();
                assert_eq!(
                    is_proper_power(&input),
                    prefix_function_power(&input),
                    "{input:?}"
                );
            }
        }
    }
    let mut random = Random(0x5eed_1234);
    for len in [
        30usize, 64, 210, 2310, 4096, 30030, 65536, 510510, 720720, 900000,
    ] {
        for period in [1usize, 2, 3, 5, 7, 11, 13, 17, 251] {
            let base = random.bytes(period, 256);
            let mut input: Vec<_> = (0..len).map(|i| base[i % period]).collect();
            assert_eq!(is_proper_power(&input), prefix_function_power(&input));
            input[len / 2] ^= 1;
            assert_eq!(is_proper_power(&input), prefix_function_power(&input));
        }
        let input = random.bytes(len, 2);
        assert_eq!(is_proper_power(&input), prefix_function_power(&input));
    }
}

#[test]
fn random_primitive_blocks_match_the_existing_sorter() {
    let mut induced = Workspace::default();
    let mut fallback = flag_sort::Workspace::default();
    let mut random = Random(0x1923_ab5d);
    for len in (2..300).chain([
        511, 512, 513, 1023, 2047, 2048, 2049, 4095, 4096, 4097, 10_000, 65_535, 65_536, 65_537,
        131_073, 300_001,
    ]) {
        for alphabet in [2, 3, 4, 7, 16, 64, 256] {
            let input = random.bytes(len, alphabet);
            if is_proper_power(&input) {
                continue;
            }
            check_induced(&mut induced, &mut fallback, &input);
            check_sort(&mut induced, &mut fallback, &input);
        }
    }
}

#[test]
fn smallest_blocks_and_constant_blocks_match_the_existing_sorter() {
    let mut induced = Workspace::default();
    let mut fallback = flag_sort::Workspace::default();
    for input in [
        &[7u8][..],
        &[0, 1],
        &[1, 0],
        &[0, 0],
        &[0, 0, 1],
        &[0, 1, 0],
        &[1, 0, 0],
        &[2, 1, 0],
        &[0, 1, 2],
        &[5, 5, 5],
    ] {
        if !is_proper_power(input) && input.len() >= 2 {
            check_induced(&mut induced, &mut fallback, input);
        }
        check_sort(&mut induced, &mut fallback, input);
    }
    for len in [1, 2, 3, 2048, 4097, 900_000] {
        for byte in [0, 0x41, 0xff] {
            check_sort(&mut induced, &mut fallback, &vec![byte; len]);
        }
    }
}

#[test]
fn proper_powers_route_to_the_existing_sorter() {
    let mut induced = Workspace::default();
    let mut fallback = flag_sort::Workspace::default();
    let mut random = Random(0x0bad_cafe);
    for period in [1usize, 2, 3, 7, 64, 251, 1000, 4500] {
        for len in [
            period * 2,
            period * 3,
            4096 / period * period,
            900_000 / period * period,
        ] {
            if len < 2 * period {
                continue;
            }
            for alphabet in [2, 256] {
                let base = random.bytes(period, alphabet);
                let input: Vec<_> = (0..len).map(|i| base[i % period]).collect();
                assert_eq!(is_proper_power(&input), prefix_function_power(&input));
                check_sort(&mut induced, &mut fallback, &input);
            }
        }
    }
}

#[test]
fn near_periodic_blocks_match_the_existing_sorter() {
    let mut induced = Workspace::default();
    let mut fallback = flag_sort::Workspace::default();
    let mut random = Random(0x7777_1111);
    for period in [1usize, 2, 7, 251, 4096] {
        for len in [4096usize, 4097, 65_536, 300_000] {
            let base = random.bytes(period, 256);
            let periodic: Vec<_> = (0..len).map(|i| base[i % period]).collect();
            for change in [0, len / 3, len - 1] {
                let mut input = periodic.clone();
                input[change] = input[change].wrapping_add(1);
                check_induced(&mut induced, &mut fallback, &input);
                check_sort(&mut induced, &mut fallback, &input);
            }
            // A period that does not divide the length leaves the block
            // primitive but maximally repetitive.
            let input = &periodic[..len - 1];
            if !is_proper_power(input) {
                check_induced(&mut induced, &mut fallback, input);
                check_sort(&mut induced, &mut fallback, input);
            }
        }
    }
}

#[test]
fn deeply_recursive_blocks_match_the_existing_sorter() {
    let mut induced = Workspace::default();
    let mut fallback = flag_sort::Workspace::default();
    // Fibonacci and Thue-Morse words recurse through many reduced levels.
    let mut fibonacci = vec![b'a'];
    let mut previous = vec![b'b'];
    while fibonacci.len() < 900_000 {
        let next = [fibonacci.as_slice(), previous.as_slice()].concat();
        previous = std::mem::replace(&mut fibonacci, next);
    }
    let thue_morse: Vec<_> = (0..900_000u32)
        .map(|i| (i.count_ones() & 1) as u8)
        .collect();
    for word in [&fibonacci, &thue_morse] {
        for len in [MIN_INDUCED, 10_946, 65_536, 317_811, 524_288, 900_000] {
            let input = &word[..len];
            if !is_proper_power(input) {
                check_induced(&mut induced, &mut fallback, input);
            }
            check_sort(&mut induced, &mut fallback, input);
        }
    }
}

#[test]
fn full_size_blocks_match_the_existing_sorter() {
    let mut induced = Workspace::default();
    let mut fallback = flag_sort::Workspace::default();
    let mut random = Random(0x9e37_79b9);
    let record = b"{\"id\":1234,\"name\":\"value\",\"tags\":[\"a\",\"b\"]}\n";
    let text: Vec<u8> = (0..900_000)
        .map(|i| record[i % record.len()] ^ u8::from(random.next().is_multiple_of(23)))
        .collect();
    for input in [random.bytes(900_000, 256), random.bytes(900_000, 4), text] {
        check_induced(&mut induced, &mut fallback, &input);
        check_sort(&mut induced, &mut fallback, &input);
    }
}

#[test]
fn shared_storage_is_reused_across_both_sorters() {
    let mut induced = Workspace::default();
    let mut random = Random(0x4242_4242);
    let large = random.bytes(900_000, 256);
    let periodic: Vec<_> = (0..900_000).map(|i| (i % 7) as u8).collect();
    induced.sort(&large).unwrap();
    induced.sort(&periodic).unwrap();
    let capacity = induced.capacity();
    let small = random.bytes(1000, 3);
    let medium = random.bytes(70_000, 256);
    for input in [&periodic, &small, &medium, &large, &periodic, &medium] {
        let mut fresh = flag_sort::Workspace::default();
        let expected = fresh.sort(input).unwrap();
        assert_eq!(induced.sort(input).unwrap(), expected);
        assert_eq!(induced.capacity(), capacity);
    }
}
