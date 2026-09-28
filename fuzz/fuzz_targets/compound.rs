#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| nfs_client::fuzzing::compound(data));
