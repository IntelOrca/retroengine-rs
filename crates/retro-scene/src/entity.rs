//! Runtime entity storage matching RSDKv4's `objectEntityList`.
//!
//! Ported from `Entity` and the object loops in `RSDKv4/Object.hpp` / `RSDKv4/Object.cpp`
//! (RSDKModding/RSDKv4-Decompilation @ a7f5195). v4 keeps `ENTITY_COUNT * 2` entity slots:
//!
//! * `0..ENTITY_COUNT` is the "regular" list scanned by the frame update loops. Scene entities
//!   are loaded starting at slot [`SCENE_ENTITY_START`] (`32`), and the last `0x80` slots from
//!   [`TEMPENTITY_START`] are used by `CreateTempObject`.
//! * `ENTITY_COUNT..ENTITY_COUNT * 2` is the "storage" list, only reachable through
//!   `CopyObject`/`ResetObjectEntity`. v4 has no separate `RESERVE_*` constant; the storage
//!   half is the reserve.
//!
//! All accesses are bounds checked: unlike upstream, an out-of-range slot yields [`None`] (or
//! is ignored) instead of undefined behaviour.

use serde::Serialize;

/// Serde helper: `[i32; 48]` needs the slice impl because serde only derives arrays up to 32.
fn serialize_values<S>(values: &[i32; 48], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    values.as_slice().serialize(serializer)
}

/// Number of `Entity` slots scanned by the frame update loops (`ENTITY_COUNT`).
pub const ENTITY_COUNT: usize = 0x4A0;
/// Number of slots in the full entity bank, including the storage half (`ENTITY_COUNT * 2`).
pub const ENTITY_SLOT_COUNT: usize = ENTITY_COUNT * 2;
/// First slot used by `CreateTempObject` (`TEMPENTITY_START`).
pub const TEMPENTITY_START: usize = ENTITY_COUNT - 0x80;
/// First slot of the storage half of the entity bank.
pub const ENTITY_STORAGE_START: usize = ENTITY_COUNT;
/// First slot a scene entity is loaded into (`objectEntityList[32]`).
pub const SCENE_ENTITY_START: usize = 32;
/// Highest object type id, excluding the blank object (`OBJECT_COUNT`).
pub const OBJECT_COUNT: usize = 0x100;
/// Number of type groups (`TYPEGROUP_COUNT`); `0` is the all-entities group.
pub const TYPEGROUP_COUNT: usize = 0x103;
/// Number of draw layers in Origins revisions (`DRAWLAYER_COUNT`).
pub const DRAWLAYER_COUNT: usize = 8;
/// Number of floor/roof sensors carried by an entity in revisions >= rev01.
pub const FLOOR_SENSOR_COUNT: usize = 5;

/// One runtime entity, mirroring upstream `struct Entity` field for field.
///
/// Upstream booleans are `byte`s; they are kept as `u8` so scripts can read/write raw values.
/// `priority` is a `PRIORITY_*` code, `direction` a `FLIP_*` code, `control_mode` a
/// `CONTROLMODE_*` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Entity {
    /// X position in 16.16 fixed point.
    pub xpos: i32,
    /// Y position in 16.16 fixed point.
    pub ypos: i32,
    /// X velocity in 16.16 fixed point.
    pub xvel: i32,
    /// Y velocity in 16.16 fixed point.
    pub yvel: i32,
    /// Ground speed in 16.16 fixed point.
    pub speed: i32,
    /// General-purpose object values (`values[48]`).
    #[serde(serialize_with = "serialize_values")]
    pub values: [i32; 48],
    /// Script state.
    pub state: i32,
    /// Ground angle.
    pub angle: i32,
    /// Scale in 16.16 fixed point.
    pub scale: i32,
    /// Rotation.
    pub rotation: i32,
    /// Alpha.
    pub alpha: i32,
    /// Animation timer.
    pub animation_timer: i32,
    /// Animation speed; `0` selects the animation's own speed.
    pub animation_speed: i32,
    /// Look position X (camera offset).
    pub look_pos_x: i32,
    /// Look position Y (camera offset).
    pub look_pos_y: i32,
    /// Custom type group id (`groupID`).
    pub group_id: u16,
    /// Object type id (index into the object list).
    pub type_id: u8,
    /// Object property value from the scene file.
    pub property_value: u8,
    /// `PRIORITY_*` update priority.
    pub priority: u8,
    /// Draw layer (`drawOrder`).
    pub draw_order: u8,
    /// `FLIP_*` facing direction.
    pub direction: u8,
    /// `INK_*` ink effect.
    pub ink_effect: u8,
    /// Current animation.
    pub animation: u8,
    /// Previous animation, used to detect animation changes.
    pub prev_animation: u8,
    /// Current animation frame.
    pub frame: u8,
    /// `CMODE_*` collision mode.
    pub collision_mode: u8,
    /// Collision plane (`CPATH_*`).
    pub collision_plane: u8,
    /// `CONTROLMODE_*` control mode.
    pub control_mode: i8,
    /// Control lock.
    pub control_lock: u8,
    /// Pushing flag.
    pub pushing: u8,
    /// Visibility.
    pub visible: u8,
    /// Whether `ProcessObjectMovement` performs tile collisions.
    pub tile_collisions: u8,
    /// Whether the entity is added to type groups (`objectInteractions`).
    pub object_interactions: u8,
    /// Whether tile collision gravity handling is active.
    pub gravity: u8,
    /// Player input up.
    pub up: u8,
    /// Player input down.
    pub down: u8,
    /// Player input left.
    pub left: u8,
    /// Player input right.
    pub right: u8,
    /// Player jump press.
    pub jump_press: u8,
    /// Player jump hold.
    pub jump_hold: u8,
    /// Camera scroll tracking mode.
    pub scroll_tracking: u8,
    /// Floor/roof sensor collision flags (left, center, right, left-center, right-center).
    pub floor_sensors: [u8; FLOOR_SENSOR_COUNT],
}

impl Default for Entity {
    fn default() -> Self {
        Self {
            xpos: 0,
            ypos: 0,
            xvel: 0,
            yvel: 0,
            speed: 0,
            values: [0; 48],
            state: 0,
            angle: 0,
            scale: 0,
            rotation: 0,
            alpha: 0,
            animation_timer: 0,
            animation_speed: 0,
            look_pos_x: 0,
            look_pos_y: 0,
            group_id: 0,
            type_id: 0,
            property_value: 0,
            priority: 0,
            draw_order: 0,
            direction: 0,
            ink_effect: 0,
            animation: 0,
            prev_animation: 0,
            frame: 0,
            collision_mode: 0,
            collision_plane: 0,
            control_mode: 0,
            control_lock: 0,
            pushing: 0,
            visible: 0,
            tile_collisions: 0,
            object_interactions: 0,
            gravity: 0,
            up: 0,
            down: 0,
            left: 0,
            right: 0,
            jump_press: 0,
            jump_hold: 0,
            scroll_tracking: 0,
            floor_sensors: [0; FLOOR_SENSOR_COUNT],
        }
    }
}

impl Entity {
    /// Zeroes the entity and applies the scene-load defaults: `drawOrder = 3`, `scale = 512`,
    /// `objectInteractions`, `visible` and `tileCollisions` set. Mirrors the loop in
    /// `LoadStageFiles`.
    #[must_use]
    pub fn scene_default() -> Self {
        Self {
            draw_order: 3,
            scale: 512,
            object_interactions: 1,
            visible: 1,
            tile_collisions: 1,
            ..Self::default()
        }
    }
}

/// A `TypeGroupList` from upstream: the entity slots matching one type or custom group, in slot
/// order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TypeGroupList {
    /// Entity slot indices, in ascending slot order.
    pub entity_refs: Vec<i32>,
}

impl TypeGroupList {
    /// Clears the list, keeping its capacity.
    pub fn clear(&mut self) {
        self.entity_refs.clear();
    }
}

/// The fixed-size entity bank (`objectEntityList[ENTITY_COUNT * 2]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityStore {
    slots: Vec<Entity>,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityStore {
    /// Creates a zeroed bank.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: vec![Entity::default(); ENTITY_SLOT_COUNT],
        }
    }

    /// Zeroes the regular half and applies the scene defaults to every regular slot, exactly like
    /// the `memset(objectEntityList, 0, ENTITY_COUNT * sizeof(Entity))` + defaults loop in
    /// `LoadStageFiles`. The storage half is left untouched, as upstream does.
    pub fn reset_scene(&mut self) {
        for slot in self.slots.iter_mut().take(ENTITY_COUNT) {
            *slot = Entity::scene_default();
        }
    }

    /// Number of slots in the bank.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the bank has no slots (never true for a real bank).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Returns the entity at `slot`.
    #[must_use]
    pub fn get(&self, slot: usize) -> Option<&Entity> {
        self.slots.get(slot)
    }

    /// Returns the entity at `slot`, or the blank entity stored at bank index 0.
    #[must_use]
    pub fn get_or_blank(&self, slot: usize) -> &Entity {
        self.slots.get(slot).unwrap_or(&self.slots[0])
    }

    /// Returns a mutable entity at `slot`.
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Entity> {
        self.slots.get_mut(slot)
    }

    /// Returns an iterator over the regular half (`0..ENTITY_COUNT`), in slot order.
    pub fn regular(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().take(ENTITY_COUNT)
    }

    /// Returns the slots of the regular half, in slot order.
    #[must_use]
    pub fn regular_len(&self) -> usize {
        ENTITY_COUNT
    }

    /// Implements `ResetObjectEntity`: zeroes `slot`, sets the common fields and the new type,
    /// property, position, `direction = FLIP_NONE`, `priority = PRIORITY_BOUNDS`, `drawOrder = 3`,
    /// `scale = 512`, `inkEffect = INK_NONE`, and enables interactions/visibility/collisions.
    ///
    /// Returns `false` when `slot` is outside the bank.
    pub fn reset_object_entity(
        &mut self,
        slot: usize,
        type_id: u8,
        property_value: u8,
        x: i32,
        y: i32,
    ) -> bool {
        let Some(entity) = self.slots.get_mut(slot) else {
            return false;
        };
        *entity = Entity {
            type_id,
            property_value,
            xpos: x,
            ypos: y,
            direction: 0,
            priority: 0,
            draw_order: 3,
            scale: 512,
            ink_effect: 0,
            object_interactions: 1,
            visible: 1,
            tile_collisions: 1,
            ..Entity::default()
        };
        true
    }

    /// Implements `CreateTempObject`: advances `cursor` past any occupied temp slot (wrapping at
    /// [`ENTITY_COUNT`] back to [`TEMPENTITY_START`]), then resets the slot with an
    /// `PRIORITY_ACTIVE` priority.
    ///
    /// `cursor` is upstream `scriptEng.arrayPosition[8]`. Returns the created slot, or `None`
    /// when `cursor` is outside the temp range.
    pub fn create_temp_object(
        &mut self,
        cursor: &mut usize,
        type_id: u8,
        property_value: u8,
        x: i32,
        y: i32,
    ) -> Option<usize> {
        if !(TEMPENTITY_START..ENTITY_COUNT).contains(cursor) {
            *cursor = TEMPENTITY_START;
        }
        if self
            .slots
            .get(*cursor)
            .is_some_and(|entity| entity.type_id > 0)
        {
            *cursor += 1;
            if *cursor == ENTITY_COUNT {
                *cursor = TEMPENTITY_START;
            }
        }
        let slot = *cursor;
        let entity = self.slots.get_mut(slot)?;
        *entity = Entity {
            type_id,
            property_value,
            xpos: x,
            ypos: y,
            direction: 0,
            priority: 1, // PRIORITY_ACTIVE
            draw_order: 3,
            scale: 512,
            ink_effect: 0,
            object_interactions: 1,
            visible: 1,
            tile_collisions: 1,
            ..Entity::default()
        };
        Some(slot)
    }

    /// Implements `CopyObject`: copies `count` consecutive entities from `src` to `dst`. Slots
    /// outside the bank are skipped (upstream would read/write out of bounds).
    ///
    /// Returns the number of entities copied.
    pub fn copy_objects(&mut self, dst: usize, src: usize, count: i32) -> usize {
        let count = usize::try_from(count).unwrap_or(0);
        let mut copied = 0;
        for index in 0..count {
            let Some(source) = self.slots.get(src.wrapping_add(index)).copied() else {
                continue;
            };
            let Some(target) = self.slots.get_mut(dst.wrapping_add(index)) else {
                continue;
            };
            *target = source;
            copied += 1;
        }
        copied
    }

    /// Builds the type groups for `flags` (upstream `processObjectFlag`), matching
    /// `ProcessObjects`'s second pass:
    ///
    /// * custom group `groupID` when `groupID >= OBJECT_COUNT`,
    /// * the entity's type group,
    /// * group `0`, the all-entities group.
    ///
    /// Group ids at or above [`TYPEGROUP_COUNT`] are ignored rather than overflowing.
    pub fn build_type_groups(
        &self,
        flags: &[bool],
        groups: &mut [TypeGroupList],
        object_count: usize,
    ) {
        for group in groups.iter_mut() {
            group.clear();
        }
        for (slot, entity) in self.slots.iter().take(ENTITY_COUNT).enumerate() {
            if !flags.get(slot).copied().unwrap_or(false) || entity.object_interactions == 0 {
                continue;
            }
            let group_id = usize::from(entity.group_id);
            if group_id >= object_count
                && let Some(list) = groups.get_mut(group_id)
            {
                list.entity_refs.push(slot as i32);
            }
            if let Some(list) = groups.get_mut(usize::from(entity.type_id)) {
                list.entity_refs.push(slot as i32);
            }
            if let Some(list) = groups.first_mut() {
                list.entity_refs.push(slot as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bank_is_zeroed_and_full() {
        let store = EntityStore::new();
        assert_eq!(store.len(), ENTITY_SLOT_COUNT);
        assert_eq!(store.get(0), Some(&Entity::default()));
        assert_eq!(store.get(ENTITY_SLOT_COUNT - 1), Some(&Entity::default()));
        assert_eq!(store.get(ENTITY_SLOT_COUNT), None);
    }

    #[test]
    fn reset_scene_applies_defaults_and_leaves_storage_untouched() {
        let mut store = EntityStore::new();
        store.get_mut(ENTITY_STORAGE_START).unwrap().xpos = 123;
        store.get_mut(0).unwrap().xpos = -1;
        store.reset_scene();
        let first = store.get(0).unwrap();
        assert_eq!(first.xpos, 0, "the regular half is zeroed before defaults");
        assert_eq!(first.draw_order, 3);
        assert_eq!(first.scale, 512);
        assert_eq!(first.object_interactions, 1);
        assert_eq!(first.visible, 1);
        assert_eq!(first.tile_collisions, 1);
        assert_eq!(
            store.get(ENTITY_STORAGE_START).unwrap().xpos,
            123,
            "upstream only memsets the regular half"
        );
    }

    #[test]
    fn reset_object_entity_sets_upstream_fields() {
        let mut store = EntityStore::new();
        assert!(store.reset_object_entity(7, 3, 9, -100, 200));
        let entity = store.get(7).unwrap();
        assert_eq!(entity.type_id, 3);
        assert_eq!(entity.property_value, 9);
        assert_eq!((entity.xpos, entity.ypos), (-100, 200));
        assert_eq!(entity.direction, 0);
        assert_eq!(entity.priority, 0);
        assert_eq!(entity.draw_order, 3);
        assert_eq!(entity.scale, 512);
        assert_eq!(entity.ink_effect, 0);
        assert_eq!(entity.object_interactions, 1);
        assert_eq!(entity.visible, 1);
        assert_eq!(entity.tile_collisions, 1);
        assert!(!store.reset_object_entity(ENTITY_SLOT_COUNT, 1, 0, 0, 0));
    }

    #[test]
    fn create_temp_object_advances_and_wraps() {
        let mut store = EntityStore::new();
        let mut cursor = TEMPENTITY_START;
        assert_eq!(
            store.create_temp_object(&mut cursor, 1, 0, 0, 0),
            Some(TEMPENTITY_START)
        );
        assert_eq!(cursor, TEMPENTITY_START);
        assert_eq!(
            store.create_temp_object(&mut cursor, 2, 0, 0, 0),
            Some(TEMPENTITY_START + 1)
        );
        assert!(store.get(TEMPENTITY_START + 1).unwrap().type_id > 0);
        // Occupy the last temp slot, then create once more: upstream increments the cursor,
        // wraps it at ENTITY_COUNT and reuses the first temp slot.
        let last = ENTITY_COUNT - 1;
        cursor = last;
        store.get_mut(last).unwrap().type_id = 9;
        assert_eq!(
            store.create_temp_object(&mut cursor, 3, 0, 0, 0),
            Some(TEMPENTITY_START)
        );
        assert_eq!(cursor, TEMPENTITY_START);
        // A cursor outside the temp range is clamped.
        let mut fresh = EntityStore::new();
        let mut bad_cursor = 0;
        assert_eq!(
            fresh.create_temp_object(&mut bad_cursor, 1, 0, 0, 0),
            Some(TEMPENTITY_START)
        );
        assert_eq!(bad_cursor, TEMPENTITY_START);
    }

    #[test]
    fn copy_objects_copies_consecutive_slots_including_storage() {
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 5, 1, 10, 20);
        store.reset_object_entity(1, 6, 2, 30, 40);
        assert_eq!(store.copy_objects(ENTITY_STORAGE_START, 0, 2), 2);
        assert_eq!(store.get(ENTITY_STORAGE_START).unwrap().type_id, 5);
        assert_eq!(store.get(ENTITY_STORAGE_START + 1).unwrap().type_id, 6);
        assert_eq!(store.copy_objects(ENTITY_SLOT_COUNT - 1, 0, 4), 1);
        assert_eq!(store.copy_objects(0, 0, -1), 0);
    }

    #[test]
    fn type_groups_follow_slot_order_and_groups() {
        let mut store = EntityStore::new();
        store.reset_object_entity(10, 4, 0, 0, 0);
        store.reset_object_entity(3, 4, 0, 0, 0);
        store.reset_object_entity(5, 7, 0, 0, 0);
        store.get_mut(5).unwrap().group_id = (OBJECT_COUNT + 2) as u16;
        store.get_mut(3).unwrap().object_interactions = 0;

        let mut flags = vec![false; ENTITY_COUNT];
        flags[10] = true;
        flags[3] = true;
        flags[5] = true;

        let mut groups = vec![TypeGroupList::default(); TYPEGROUP_COUNT];
        store.build_type_groups(&flags, &mut groups, OBJECT_COUNT);
        assert_eq!(groups[0].entity_refs, vec![5, 10]);
        assert_eq!(groups[4].entity_refs, vec![10]);
        assert_eq!(groups[7].entity_refs, vec![5]);
        assert_eq!(groups[OBJECT_COUNT + 2].entity_refs, vec![5]);
    }

    #[test]
    fn out_of_range_group_ids_are_ignored() {
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 0, 0);
        store.get_mut(0).unwrap().group_id = u16::MAX;
        let mut groups = vec![TypeGroupList::default(); TYPEGROUP_COUNT];
        store.build_type_groups(&[true], &mut groups, OBJECT_COUNT);
        assert_eq!(groups[0].entity_refs, vec![0]);
        assert_eq!(groups[1].entity_refs, vec![0]);
    }
}
