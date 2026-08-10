#![no_main]

use jello::{parse, repair_json5, RepairOutcome};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Ok(source) = std::str::from_utf8(input) else {
        return;
    };

    let _ = jello::parse_json5(source);
    let _ = jello::plan_repair_json5(source);

    match repair_json5(source) {
        RepairOutcome::Valid(result) | RepairOutcome::Repaired(result) => {
            assert!(parse(&result.output).is_ok());
        }
        RepairOutcome::Unrepairable(_) => {}
    }
});
