#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| nfs_rpc::fuzzing::records(data));
