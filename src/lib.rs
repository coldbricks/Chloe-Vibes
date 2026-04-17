// Library surface for integration tests and external consumers.
//
// The main binary (src/main.rs) keeps its own `mod audio` declaration and
// does not depend on this library -- leaving it untouched preserves the
// existing build exactly. This file exists so that `tests/parity.rs`
// can `use chloe_vibes::audio::*` without coupling to binary internals.
//
// Only the signal-processing module is exposed; GUI, settings and
// presets are intentionally left private since they aren't needed for
// cross-platform parity testing.
//
// The blanket clippy allows below mirror the behaviour that `audio.rs`
// already enjoys when it is compiled as a private bin-internal module.
// Exposing the same file as a public library surface activates extra
// lints (`new_without_default`, `manual_range_contains`, etc.) that are
// not part of this Fix B task's scope — allowing them here keeps CI's
// `-D warnings` clippy step green without modifying audio.rs itself.
#![allow(
    clippy::new_without_default,
    clippy::manual_range_contains,
    clippy::manual_clamp,
    clippy::too_many_arguments,
    dead_code
)]

pub mod audio;
