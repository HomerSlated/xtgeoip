//! xtgeoip © Haze N Sparkle 2026 (MIT)
//!
//! The library half of the crate. This exists so that code can be *shared*
//! rather than duplicated, not so that anything is published: the target is a
//! plain `rlib`, a compile-time archive statically linked into the binaries.
//! Nothing new is deployed — `ldd target/release/xtgeoip` is unchanged, and
//! packaging still installs the binary, the man page and the example config.
//!
//! Why it exists at all, after being deferred in
//! `docs/design/spec-driven-validator.md` §6 (2026-06-08) on the grounds that
//! it "would only enable external/integration test crates". Four months of
//! evidence says otherwise:
//!
//! * `xtgeoip-docgen` cannot call `plan()` or `normalize_cli_to_action`, so
//!   #92's generation-side validator is impossible for anything semantic.
//! * `tests/` is an empty directory, because nothing could import a binary.
//! * `is_root()` is duplicated between `main.rs` and `xtgeoip-tests.rs`.
//! * Unit tests live *inside* guardian-signed files, so editing a test
//!   invalidates a security signature (see #100, and TODO.md on #99).
//!
//! The deferral was parked on "decide it on #88 grounds", and #88 was later
//! closed as premise-invalidated — so the criterion it waited for no longer
//! existed.

pub mod action;
pub mod backup;
pub mod build;
pub mod cli;
pub mod conf;
pub mod config;
pub mod fetch;
pub mod generated;
pub mod messages;
pub mod secrets;
pub mod spec;
pub mod version;
