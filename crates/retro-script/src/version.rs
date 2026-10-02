//! Script bytecode versions and RSDKv4 revision gating.
//!
//! Upstream RSDKv4 compiles the same source language against four revisions of the runtime
//! (`RETRO_REV00` .. `RETRO_REV03`, see `RetroEngine.hpp`). The encoded opcode index of an
//! operation and the id of a variable depend on the revision because revision-gated entries
//! shift everything that follows them. Revision tables are runtime data; this crate never uses
//! conditional compilation to pick a revision.
//!
//! Only the v4 tables are ported so far. Requests for [`ScriptVersion::V2`] or
//! [`ScriptVersion::V3`] currently resolve to the v4 tables (see [`crate::opcodes`]).

use serde::Serialize;

/// Script bytecode version. RSDKv2/v3 are placeholders until those VMs are ported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ScriptVersion {
    V2,
    V3,
    V4,
}

/// RSDKv4 engine revision, matching upstream `RETRO_REV00`..`RETRO_REV03`.
///
/// * `Rev00`: early Sonic 1 releases.
/// * `Rev01`: early Sonic 2 releases (adds `MatrixInverse`).
/// * `Rev02`: S3&K proof of concept / Sega Forever S1+S2 (drops `LoadFontFile` and `DrawText`,
///   adds `GetObjectValue`/`SetObjectValue`/`CopyObject`, changes `LoadTextFile` and
///   `SetPaletteFade` operand counts).
/// * `Rev03`: Sonic Origins (adds the screen/input extras at the end of the opcode enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum V4Revision {
    Rev00,
    Rev01,
    Rev02,
    Rev03,
}

impl V4Revision {
    /// Revision tables in order, useful for exhaustive tests.
    pub const ALL: [V4Revision; 4] = [
        V4Revision::Rev00,
        V4Revision::Rev01,
        V4Revision::Rev02,
        V4Revision::Rev03,
    ];

    /// Zero-based revision index (0..=3).
    pub const fn index(self) -> usize {
        match self {
            V4Revision::Rev00 => 0,
            V4Revision::Rev01 => 1,
            V4Revision::Rev02 => 2,
            V4Revision::Rev03 => 3,
        }
    }

    /// Lower-case upstream name (`"rev00"`..`"rev03"`).
    pub const fn name(self) -> &'static str {
        match self {
            V4Revision::Rev00 => "rev00",
            V4Revision::Rev01 => "rev01",
            V4Revision::Rev02 => "rev02",
            V4Revision::Rev03 => "rev03",
        }
    }
}

impl ScriptVersion {
    /// Revision table used for this script version. v2/v3 are not ported yet, so they fall back
    /// to the requested v4 revision; this keeps the API usable without pretending the v2/v3
    /// opcode sets exist.
    pub const fn v4_revision(self, revision: V4Revision) -> V4Revision {
        match self {
            ScriptVersion::V2 | ScriptVersion::V3 | ScriptVersion::V4 => revision,
        }
    }

    /// Lower-case name (`"v2"`, `"v3"`, `"v4"`).
    pub const fn name(self) -> &'static str {
        match self {
            ScriptVersion::V2 => "v2",
            ScriptVersion::V3 => "v3",
            ScriptVersion::V4 => "v4",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_indices_are_ordered() {
        assert_eq!(V4Revision::Rev00.index(), 0);
        assert_eq!(V4Revision::Rev03.index(), 3);
        assert_eq!(V4Revision::ALL.len(), 4);
    }
}
