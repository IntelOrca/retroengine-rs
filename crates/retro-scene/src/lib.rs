//! Runtime scene model for the RSDKv4 legacy engine.
//!
//! This crate owns the parts of the engine that persist across frames and are not script VM or
//! platform concerns:
//!
//! * [`entity`] — the fixed-size entity bank (`objectEntityList`), type groups and the
//!   `ResetObjectEntity`/`CreateTempObject`/`CopyObject` primitives.
//! * [`objects`] — the merged object/script list built from `GameConfig.bin` + `StageConfig.bin`.
//! * [`stage`] — the 128x128 chunk layout and stage runtime globals.
//! * [`camera`] — camera and screen globals.
//! * [`collision`] — `RSDKv4/Collision.cpp` tile/object collision routines.
//! * [`math`] — the sin/cos/atan lookup tables.
//!
//! Everything here is deterministic and performs no I/O.
#![forbid(unsafe_code)]

pub mod camera;
pub mod collision;
pub mod entity;
pub mod math;
pub mod objects;
pub mod stage;

pub use camera::{Camera, Screen};
pub use collision::{
    C_BOX, C_PLATFORM, C_SOLID, C_SOLID2, C_TOUCH, CMODE_FLOOR, CMODE_LWALL, CMODE_ROOF,
    CMODE_RWALL, CSIDE_FLOOR, CSIDE_LENTITY, CSIDE_LWALL, CSIDE_RENTITY, CSIDE_ROOF, CSIDE_RWALL,
    CollisionSensor, FLIP_NONE, FLIP_X, FLIP_XY, FLIP_Y, SOLID_ALL, SOLID_LRB, SOLID_NONE,
    SOLID_TOP, SceneCollision,
};
pub use entity::EntityStore;
pub use entity::{
    DRAWLAYER_COUNT, ENTITY_COUNT, ENTITY_SLOT_COUNT, ENTITY_STORAGE_START, Entity,
    FLOOR_SENSOR_COUNT, OBJECT_COUNT, SCENE_ENTITY_START, TEMPENTITY_START, TYPEGROUP_COUNT,
    TypeGroupList,
};
pub use math::MathTables;
pub use objects::{ObjectEntry, ObjectRegistry, strip_spaces};
pub use stage::{CHUNK_SIZE, StageLayout, StageState, TILE_COUNT};
