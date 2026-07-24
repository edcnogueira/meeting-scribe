//! Shared test-support code for the diarization evaluation harness.
//!
//! Included from integration test crates via `mod support;`. Files under
//! `tests/support/` are compiled as modules of the including test binary, not
//! as standalone test binaries, so this code is shared without being run on its
//! own.

// Not every helper is exercised by every test binary that includes this module.
#![allow(dead_code)]

pub mod fixtures;
