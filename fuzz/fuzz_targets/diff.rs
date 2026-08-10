#![no_main]

use jello::unified_diff;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Ok(source) = std::str::from_utf8(input) else {
        return;
    };
    let split = source
        .char_indices()
        .nth(source.chars().count() / 2)
        .map_or(source.len(), |(index, _)| index);
    let (left, right) = source.split_at(split);
    let _ = unified_diff(left, right, 3);
});
