// auto-generated
#![allow(dead_code)]
use crate::generated::error_text;

// Flag bits, in sorted flag-universe order.
pub const B: u8 = 1 << 0;
pub const C: u8 = 1 << 1;
pub const F: u8 = 1 << 2;
pub const L: u8 = 1 << 3;
pub const P: u8 = 1 << 4;

/// One combination guard: fires when every `require` bit is present and
/// no `forbid` bit is. First firing guard per context wins (= precedence).
pub struct Guard {
    pub require: u8,
    pub forbid: u8,
    pub key: &'static str,
    pub message: &'static str,
}

pub const TOP_LEVEL_GUARDS: &[Guard] = &[
    Guard { require: L, forbid: 0, key: "top_level_legacy", message: error_text::NO_LEGACY_HERE },
    Guard { require: P, forbid: B | C, key: "top_level_prune_no_target", message: error_text::NO_PRUNE_ALONE },
    Guard { require: F, forbid: B | C, key: "top_level_force_no_target", message: error_text::NO_FORCE_ALONE },
    Guard { require: C | P | F, forbid: B, key: "top_level_prune_clean_force", message: error_text::NO_PRUNE_CLEAN_FORCE },
    Guard { require: C | P, forbid: B, key: "top_level_prune_with_clean", message: error_text::NO_PRUNE_CLEAN },
    Guard { require: B | P | F, forbid: 0, key: "top_level_prune_force", message: error_text::NO_PRUNE_FORCE },
    Guard { require: B | C | F, forbid: 0, key: "top_level_force_ambiguous", message: error_text::FORCE_AMBIGUOUS },
];

pub const BUILD_GUARDS: &[Guard] = &[
    Guard { require: F, forbid: B | C, key: "build_force_no_target", message: error_text::NO_BUILD_FORCE },
    Guard { require: P, forbid: B, key: "build_prune_no_backup", message: error_text::NO_PRUNE_BACKUP },
    Guard { require: P | F, forbid: 0, key: "build_prune_force", message: error_text::NO_PRUNE_FORCE },
    Guard { require: B | C | F, forbid: 0, key: "build_force_ambiguous", message: error_text::FORCE_AMBIGUOUS },
];

pub const FETCH_GUARDS: &[Guard] = &[
    Guard { require: L, forbid: 0, key: "fetch_no_legacy", message: error_text::NO_FETCH_LEGACY },
    Guard { require: B, forbid: 0, key: "fetch_no_backup", message: error_text::NO_FETCH_BACKUP },
    Guard { require: C, forbid: 0, key: "fetch_no_clean", message: error_text::NO_FETCH_CLEAN },
    Guard { require: F, forbid: 0, key: "fetch_no_force", message: error_text::NO_FETCH_FORCE },
];

pub const RUN_GUARDS: &[Guard] = &[
    Guard { require: F, forbid: B | C, key: "run_force_no_target", message: error_text::NO_RUN_FORCE },
    Guard { require: P | F, forbid: 0, key: "run_prune_force", message: error_text::NO_PRUNE_FORCE },
    Guard { require: B | C | P, forbid: 0, key: "run_prune_ambiguous", message: error_text::PRUNE_TARGET_AMBIGUOUS },
    Guard { require: B | C | F, forbid: 0, key: "run_force_ambiguous", message: error_text::FORCE_AMBIGUOUS },
];

