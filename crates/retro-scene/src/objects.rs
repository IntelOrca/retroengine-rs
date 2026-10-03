//! Object list and script event registry.
//!
//! Mirrors the merged `objectScriptList` the engine builds in `LoadStageFiles`
//! (`RSDKv4/Scene.cpp`, RSDKModding/RSDKv4-Decompilation @ a7f5195):
//!
//! * Slot `0` is always `BlankObject`.
//! * The global objects from `GameConfig.bin` occupy `1..=global_count` when the stage's
//!   `StageConfig.bin` sets `load_global_objects`.
//! * The stage objects follow, so scene entity type ids equal positions in this list and are
//!   absolute regardless of whether globals were loaded.
//!
//! Object names are stored with spaces stripped, matching upstream `SetObjectTypeName`.

use serde::Serialize;

use retro_script::{EMPTY_EVENT, JUMPTABLE_COUNT, ObjectScript, ScriptPtr};

/// Upstream `ClearScriptData` leaves an object event with no script pointing at the sentinel
/// `SCRIPTCODE_COUNT - 1` / `JUMPTABLE_COUNT - 1`, so the engine's `scriptCode[ptr] > 0` guard
/// skips it. A zeroed pointer would alias whatever script happens to start at code position 0.
fn empty_object_script() -> ObjectScript {
    let ptr = ScriptPtr {
        code_pos: EMPTY_EVENT,
        jump_pos: JUMPTABLE_COUNT as u32 - 1,
    };
    ObjectScript {
        update: ptr,
        draw: ptr,
        startup: ptr,
    }
}

/// One object type in the merged list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ObjectEntry {
    /// Space-stripped object name (`PlayerObject`).
    pub name: String,
    /// Script variable names registered by this object (`ObjectScript` values).
    pub variable_names: Vec<String>,
    /// Event entry points into the engine's merged script file.
    pub script: ObjectScript,
    /// Index of the `.ani` file assigned by `LoadAnimation`, when implemented by the host.
    pub animation_file: Option<usize>,
    /// Sprite sheet id assigned by `LoadSpriteSheet`, when implemented by the host.
    pub sprite_sheet_id: i32,
}

/// The merged object list, indexed by type id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectRegistry {
    objects: Vec<ObjectEntry>,
}

impl Default for ObjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectRegistry {
    /// Creates a registry containing only `BlankObject` at slot `0`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            objects: vec![ObjectEntry {
                name: "BlankObject".to_owned(),
                script: empty_object_script(),
                ..ObjectEntry::default()
            }],
        }
    }

    /// Appends one object and returns its type id.
    ///
    /// The list is capped at [`crate::entity::OBJECT_COUNT`] entries (including `BlankObject`)
    /// like the engine's `objectScriptList`; a push beyond the cap returns `0` without
    /// registering anything rather than truncating the type id.
    pub fn push(
        &mut self,
        name: impl Into<String>,
        variable_names: Vec<String>,
        script: ObjectScript,
    ) -> u8 {
        if self.objects.len() >= crate::entity::OBJECT_COUNT {
            return 0;
        }
        let id = self.objects.len() as u8;
        let name = name.into();
        self.objects.push(ObjectEntry {
            name: strip_spaces(&name),
            variable_names,
            script,
            ..ObjectEntry::default()
        });
        id
    }

    /// Number of registered object types, including `BlankObject`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether only `BlankObject` is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.len() <= 1
    }

    /// Returns the object with `type_id`, if any.
    #[must_use]
    pub fn get(&self, type_id: usize) -> Option<&ObjectEntry> {
        self.objects.get(type_id)
    }

    /// Returns the mutable object with `type_id`, if any.
    pub fn get_mut(&mut self, type_id: usize) -> Option<&mut ObjectEntry> {
        self.objects.get_mut(type_id)
    }

    /// Iterates the objects in type-id order.
    pub fn iter(&self) -> impl Iterator<Item = &ObjectEntry> {
        self.objects.iter()
    }

    /// Iterates `(type_id, entry)` pairs in type-id order.
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (usize, &ObjectEntry)> {
        self.objects.iter().enumerate()
    }

    /// Finds an object type id by its space-stripped name.
    #[must_use]
    pub fn type_id(&self, name: &str) -> Option<u8> {
        let stripped = strip_spaces(name);
        self.objects
            .iter()
            .position(|entry| entry.name == stripped)
            .map(|index| index as u8)
    }
}

/// Removes spaces from an object name, matching upstream `SetObjectTypeName`.
#[must_use]
pub fn strip_spaces(name: &str) -> String {
    name.chars().filter(|character| *character != ' ').collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_script::ScriptPtr;

    fn script(code: u32) -> ObjectScript {
        ObjectScript {
            update: ScriptPtr {
                code_pos: code,
                jump_pos: 0,
            },
            ..ObjectScript::default()
        }
    }

    #[test]
    fn blank_object_is_always_slot_zero() {
        let registry = ObjectRegistry::new();
        assert_eq!(registry.len(), 1);
        assert!(registry.is_empty());
        assert_eq!(registry.get(0).unwrap().name, "BlankObject");
        assert_eq!(registry.type_id("BlankObject"), Some(0));
    }

    #[test]
    fn blank_object_events_use_the_upstream_sentinels() {
        // A zeroed event pointer would alias whatever script starts at code position 0, so the
        // blank object must point past the end of the script arrays like `ClearScriptData`.
        let registry = ObjectRegistry::new();
        let script = registry.get(0).unwrap().script;
        for pointer in [script.update, script.draw, script.startup] {
            assert_eq!(pointer.code_pos, EMPTY_EVENT);
            assert_eq!(pointer.jump_pos, JUMPTABLE_COUNT as u32 - 1);
        }
    }

    #[test]
    fn push_assigns_list_positions_in_order() {
        let mut registry = ObjectRegistry::new();
        assert_eq!(registry.push("Player Object", vec![], script(10)), 1);
        assert_eq!(registry.push("Ring", vec![], script(20)), 2);
        assert_eq!(registry.len(), 3);
        assert_eq!(registry.get(1).unwrap().name, "PlayerObject");
        assert_eq!(registry.type_id("Player Object").unwrap(), 1);
        assert_eq!(registry.get(2).unwrap().script.update.code_pos, 20);
        assert_eq!(registry.get(3), None);
    }

    #[test]
    fn global_then_stage_ordering_matches_engine_type_ids() {
        let mut registry = ObjectRegistry::new();
        for name in ["GlobalA", "GlobalB"] {
            registry.push(name, vec![], script(1));
        }
        // Stage object type ids continue after the globals.
        assert_eq!(registry.push("StageA", vec![], script(2)), 3);
        assert_eq!(registry.push("StageB", vec![], script(3)), 4);
        assert_eq!(registry.len(), 5);
    }

    #[test]
    fn stage_only_ordering_starts_at_one() {
        let mut registry = ObjectRegistry::new();
        assert_eq!(registry.push("StageA", vec![], script(2)), 1);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn push_caps_at_the_engine_object_count() {
        let mut registry = ObjectRegistry::new();
        for index in 0..crate::entity::OBJECT_COUNT - 1 {
            assert_eq!(
                registry.push(format!("Object{index}"), vec![], script(1)),
                (index + 1) as u8
            );
        }
        assert_eq!(registry.len(), crate::entity::OBJECT_COUNT);
        assert_eq!(registry.push("Overflow", vec![], script(1)), 0);
        assert_eq!(registry.len(), crate::entity::OBJECT_COUNT);
    }

    #[test]
    fn strip_spaces_matches_upstream_type_names() {
        assert_eq!(strip_spaces("Player Object"), "PlayerObject");
        assert_eq!(strip_spaces("GHZ Bridge"), "GHZBridge");
        assert_eq!(strip_spaces("NoSpaces"), "NoSpaces");
    }
}
