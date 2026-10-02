//! Parser for RSDKv4 scene files (`Data/Stages/<folder>/ActN.bin`).
//!
//! On-disk order (RSDKv4-Decompilation `LoadActLayout` in `RSDKv4/Scene.cpp`; identical in
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`):
//!
//! ```text
//! string   title card text
//! u8[4]    active tile layer flags
//! u8       tile layer mid point
//! u8       layout width in 16x16 tiles
//! u8       unused
//! u8       layout height in 16x16 tiles
//! u8       unused
//! u16[]    little-endian tile indices, row-major (width * height entries)
//! u16      entity count (little-endian)
//! repeat entity count times:
//!     u16      attribute word ("active" bits)
//!     u8       object type id
//!     u8       property value
//!     i32      x position (little-endian)
//!     i32      y position (little-endian)
//!     ...      optional fields, present when the matching attribute bit is set,
//!              always in this order: state i32 (0x1), direction u8 (0x2), scale i32 (0x4),
//!              rotation i32 (0x8), draw order u8 (0x10), priority u8 (0x20), alpha u8 (0x40),
//!              animation u8 (0x80), animation speed i32 (0x100), frame u8 (0x200),
//!              ink effect u8 (0x400), values[0..3] i32 (0x800, 0x1000, 0x2000, 0x4000)
//! ```
//!
//! The engine keeps the layout in a fixed `0x100`-wide chunk buffer and writes
//! `tiles[y * TILELAYER_CHUNK_H + x]`; this parser stores the same values compactly, row-major
//! with stride [`Scene::width`].
//!
//! Unlike the streaming loader, which simply stops reading at the end of the entity list, these
//! parsers report trailing bytes: [`Scene::from_bytes`] rejects them and
//! [`Scene::from_bytes_allow_trailing`] ignores them the way the engine does.

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use retro_io::DataSource;

/// Number of bytes reserved for the active layer flags.
pub const ACTIVE_LAYER_COUNT: usize = 4;
/// Object count above which upstream logs a warning (the entity pool holds 0x400).
pub const ENTITY_WARNING_LIMIT: usize = 0x400;
/// Layout width of the engine's tile layer buffer (`TILELAYER_CHUNK_W`).
pub const TILE_LAYER_CHUNK_WIDTH: usize = 0x100;
/// Layout height of the engine's tile layer buffer (`TILELAYER_CHUNK_H`).
pub const TILE_LAYER_CHUNK_HEIGHT: usize = 0x100;

/// Attribute bit: `state` (`i32`) is present.
pub const ENTITY_ATTRIB_STATE: u16 = 0x0001;
/// Attribute bit: `direction` (`u8`) is present.
pub const ENTITY_ATTRIB_DIRECTION: u16 = 0x0002;
/// Attribute bit: `scale` (`i32`) is present.
pub const ENTITY_ATTRIB_SCALE: u16 = 0x0004;
/// Attribute bit: `rotation` (`i32`) is present.
pub const ENTITY_ATTRIB_ROTATION: u16 = 0x0008;
/// Attribute bit: `draw_order` (`u8`) is present.
pub const ENTITY_ATTRIB_DRAW_ORDER: u16 = 0x0010;
/// Attribute bit: `priority` (`u8`) is present.
pub const ENTITY_ATTRIB_PRIORITY: u16 = 0x0020;
/// Attribute bit: `alpha` (`u8`) is present.
pub const ENTITY_ATTRIB_ALPHA: u16 = 0x0040;
/// Attribute bit: `animation` (`u8`) is present.
pub const ENTITY_ATTRIB_ANIMATION: u16 = 0x0080;
/// Attribute bit: `animation_speed` (`i32`) is present.
pub const ENTITY_ATTRIB_ANIMATION_SPEED: u16 = 0x0100;
/// Attribute bit: `frame` (`u8`) is present.
pub const ENTITY_ATTRIB_FRAME: u16 = 0x0200;
/// Attribute bit: `ink_effect` (`u8`) is present.
pub const ENTITY_ATTRIB_INK_EFFECT: u16 = 0x0400;
/// Attribute bits for `values[0]` through `values[3]`, all `i32`.
pub const ENTITY_ATTRIB_VALUES: [u16; 4] = [0x0800, 0x1000, 0x2000, 0x4000];

/// One scene entity exactly as serialised in the act file.
///
/// The `Option` fields are `Some` for every attribute bit set on the entity; `values` contains
/// only the serialised entries of `values[0..3]`, in slot order. Use [`SceneEntity::value`] to
/// look a slot up by index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneEntity {
    /// Raw attribute word; drives which optional fields were stored (`attribs`/`activeVars`).
    pub attributes: u16,
    /// Object type id.
    pub type_id: u8,
    /// Object property value.
    pub property_value: u8,
    /// X position in pixels.
    pub x: i32,
    /// Y position in pixels.
    pub y: i32,
    /// Script state (attribute [`ENTITY_ATTRIB_STATE`]).
    pub state: Option<i32>,
    /// Facing direction (attribute [`ENTITY_ATTRIB_DIRECTION`]).
    pub direction: Option<u8>,
    /// Scale (attribute [`ENTITY_ATTRIB_SCALE`]).
    pub scale: Option<i32>,
    /// Rotation (attribute [`ENTITY_ATTRIB_ROTATION`]).
    pub rotation: Option<i32>,
    /// Draw order (attribute [`ENTITY_ATTRIB_DRAW_ORDER`]).
    pub draw_order: Option<u8>,
    /// Priority (attribute [`ENTITY_ATTRIB_PRIORITY`]).
    pub priority: Option<u8>,
    /// Alpha (attribute [`ENTITY_ATTRIB_ALPHA`]).
    pub alpha: Option<u8>,
    /// Animation id (attribute [`ENTITY_ATTRIB_ANIMATION`]).
    pub animation: Option<u8>,
    /// Animation speed (attribute [`ENTITY_ATTRIB_ANIMATION_SPEED`]).
    pub animation_speed: Option<i32>,
    /// Animation frame (attribute [`ENTITY_ATTRIB_FRAME`]).
    pub frame: Option<u8>,
    /// Ink effect (attribute [`ENTITY_ATTRIB_INK_EFFECT`]).
    pub ink_effect: Option<u8>,
    /// Present object values, in slot order.
    pub values: Vec<i32>,
    /// Number of bytes of optional entity data that followed `x`/`y` (all `Option` fields and
    /// `values`, including their attribute-driven gaps in the stream).
    pub raw_values_len: usize,
}

impl SceneEntity {
    /// Returns `values[index]` when the matching attribute bit is set.
    pub fn value(&self, index: usize) -> Option<i32> {
        let attribute = *ENTITY_ATTRIB_VALUES.get(index)?;
        if self.attributes & attribute == 0 {
            return None;
        }
        let slot = ENTITY_ATTRIB_VALUES[..index]
            .iter()
            .filter(|bit| self.attributes & **bit != 0)
            .count();
        self.values.get(slot).copied()
    }

    /// Whether the entity carries the given attribute bit.
    pub fn has(&self, attribute: u16) -> bool {
        self.attributes & attribute != 0
    }
}

/// Parsed `ActN.bin` scene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scene {
    /// Title card text (upstream `titleCardText`).
    pub title: String,
    /// Active tile layer flags (upstream `activeTileLayers`).
    pub active_layers: [u8; ACTIVE_LAYER_COUNT],
    /// Tile layer mid point (upstream `tLayerMidPoint`).
    pub mid_point: u8,
    /// Layout width in 16x16 tiles (upstream `stageLayouts[0].xsize`).
    pub width: u8,
    /// Layout height in 16x16 tiles (upstream `stageLayouts[0].ysize`).
    pub height: u8,
    /// Tile indices, row-major with stride [`Scene::width`].
    pub layout: Vec<u16>,
    /// Scene entities in file order.
    pub entities: Vec<SceneEntity>,
}

impl Scene {
    /// Parses an act file from memory, rejecting trailing bytes.
    ///
    /// The engine stops reading after the last entity, so files with extra data are accepted by
    /// [`Scene::from_bytes_allow_trailing`]; this entry point reports them instead.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::parse(bytes, true)
    }

    /// Parses an act file from memory, ignoring any bytes after the last entity.
    pub fn from_bytes_allow_trailing(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::parse(bytes, false)
    }

    fn parse(bytes: &[u8], strict: bool) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);

        let title = reader.read_string()?;
        let active_layers = reader.read_array::<ACTIVE_LAYER_COUNT>()?;
        let mid_point = reader.read_u8()?;

        let width = reader.read_u8()?;
        let _unused_width = reader.read_u8()?;
        let height = reader.read_u8()?;
        let _unused_height = reader.read_u8()?;

        let tile_count = usize::from(width) * usize::from(height);
        let mut layout = Vec::with_capacity(tile_count);
        for _ in 0..tile_count {
            layout.push(reader.read_u16_le()?);
        }

        let entity_count = usize::from(reader.read_u16_le()?);
        let mut entities = Vec::with_capacity(entity_count);
        for _ in 0..entity_count {
            let attributes = reader.read_u16_le()?;
            let type_id = reader.read_u8()?;
            let property_value = reader.read_u8()?;
            let x = reader.read_i32_le()?;
            let y = reader.read_i32_le()?;
            let payload_start = reader.position();

            let state = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_STATE != 0,
                |reader| reader.read_i32_le(),
            )?;
            let direction = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_DIRECTION != 0,
                |reader| reader.read_u8(),
            )?;
            let scale = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_SCALE != 0,
                |reader| reader.read_i32_le(),
            )?;
            let rotation = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_ROTATION != 0,
                |reader| reader.read_i32_le(),
            )?;
            let draw_order = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_DRAW_ORDER != 0,
                |reader| reader.read_u8(),
            )?;
            let priority = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_PRIORITY != 0,
                |reader| reader.read_u8(),
            )?;
            let alpha = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_ALPHA != 0,
                |reader| reader.read_u8(),
            )?;
            let animation = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_ANIMATION != 0,
                |reader| reader.read_u8(),
            )?;
            let animation_speed = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_ANIMATION_SPEED != 0,
                |reader| reader.read_i32_le(),
            )?;
            let frame = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_FRAME != 0,
                |reader| reader.read_u8(),
            )?;
            let ink_effect = read_if(
                &mut reader,
                attributes & ENTITY_ATTRIB_INK_EFFECT != 0,
                |reader| reader.read_u8(),
            )?;

            let mut values = Vec::with_capacity(ENTITY_ATTRIB_VALUES.len());
            for attribute in ENTITY_ATTRIB_VALUES {
                if attributes & attribute != 0 {
                    values.push(reader.read_i32_le()?);
                }
            }

            entities.push(SceneEntity {
                attributes,
                type_id,
                property_value,
                x,
                y,
                state,
                direction,
                scale,
                rotation,
                draw_order,
                priority,
                alpha,
                animation,
                animation_speed,
                frame,
                ink_effect,
                values,
                raw_values_len: reader.position() - payload_start,
            });
        }

        if strict && !reader.is_empty() {
            return Err(FormatError::invalid(format!(
                "{} trailing bytes after scene entities",
                reader.remaining()
            )));
        }

        Ok(Self {
            title,
            active_layers,
            mid_point,
            width,
            height,
            layout,
            entities,
        })
    }

    /// Reads `<stage_dir>/Act<act>.bin` through `src` and parses it.
    ///
    /// `stage_dir` is a path relative to the source root, e.g. `Data/Stages/Zone01` (a trailing
    /// slash is accepted); `act` is the act id without the `Act` prefix or `.bin` suffix, e.g.
    /// `"1"` or `"B"`.
    pub fn load(stage_dir: &str, act: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(&Self::path(stage_dir, act))?)
    }

    /// Returns the asset path of an act file for [`Scene::load`].
    pub fn path(stage_dir: &str, act: &str) -> String {
        let directory = stage_dir.trim_end_matches('/');
        if directory.is_empty() {
            format!("Act{act}.bin")
        } else {
            format!("{directory}/Act{act}.bin")
        }
    }
}

fn read_if<T>(
    reader: &mut Reader<'_>,
    present: bool,
    read: impl FnOnce(&mut Reader<'_>) -> Result<T, FormatError>,
) -> Result<Option<T>, FormatError> {
    if present {
        read(reader).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    struct EntityFixture {
        attributes: u16,
        type_id: u8,
        property_value: u8,
        x: i32,
        y: i32,
        fields: Vec<u8>,
        values: Vec<u8>,
    }

    impl EntityFixture {
        fn empty() -> Self {
            Self {
                attributes: 0,
                type_id: 0,
                property_value: 0,
                x: 0,
                y: 0,
                fields: Vec::new(),
                values: Vec::new(),
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&self.attributes.to_le_bytes());
            bytes.push(self.type_id);
            bytes.push(self.property_value);
            bytes.extend_from_slice(&self.x.to_le_bytes());
            bytes.extend_from_slice(&self.y.to_le_bytes());
            bytes.extend_from_slice(&self.fields);
            bytes.extend_from_slice(&self.values);
            bytes
        }
    }

    struct SceneFixture {
        title: String,
        active_layers: [u8; ACTIVE_LAYER_COUNT],
        mid_point: u8,
        width: u8,
        height: u8,
        layout: Vec<u16>,
        entities: Vec<EntityFixture>,
    }

    impl SceneFixture {
        fn minimal() -> Self {
            Self {
                title: "TEST".to_owned(),
                active_layers: [1, 9, 0, 0],
                mid_point: 3,
                width: 1,
                height: 1,
                layout: vec![0],
                entities: Vec::new(),
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.push(self.title.len() as u8);
            bytes.extend_from_slice(self.title.as_bytes());
            bytes.extend_from_slice(&self.active_layers);
            bytes.push(self.mid_point);
            bytes.push(self.width);
            bytes.push(0);
            bytes.push(self.height);
            bytes.push(0);
            for tile in &self.layout {
                bytes.extend_from_slice(&tile.to_le_bytes());
            }
            bytes.extend_from_slice(&(self.entities.len() as u16).to_le_bytes());
            for entity in &self.entities {
                bytes.extend_from_slice(&entity.build());
            }
            bytes
        }
    }

    fn push_i32(bytes: &mut Vec<u8>, value: i32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn parses_minimal_scene() {
        let scene = Scene::from_bytes(&SceneFixture::minimal().build()).unwrap();
        assert_eq!(scene.title, "TEST");
        assert_eq!(scene.active_layers, [1, 9, 0, 0]);
        assert_eq!(scene.mid_point, 3);
        assert_eq!((scene.width, scene.height), (1, 1));
        assert_eq!(scene.layout, [0]);
        assert!(scene.entities.is_empty());
    }

    #[test]
    fn parses_entity_with_all_attributes() {
        let attributes = ENTITY_ATTRIB_STATE
            | ENTITY_ATTRIB_DIRECTION
            | ENTITY_ATTRIB_SCALE
            | ENTITY_ATTRIB_ROTATION
            | ENTITY_ATTRIB_DRAW_ORDER
            | ENTITY_ATTRIB_PRIORITY
            | ENTITY_ATTRIB_ALPHA
            | ENTITY_ATTRIB_ANIMATION
            | ENTITY_ATTRIB_ANIMATION_SPEED
            | ENTITY_ATTRIB_FRAME
            | ENTITY_ATTRIB_INK_EFFECT
            | ENTITY_ATTRIB_VALUES[0]
            | ENTITY_ATTRIB_VALUES[1]
            | ENTITY_ATTRIB_VALUES[2]
            | ENTITY_ATTRIB_VALUES[3];
        let mut entity = EntityFixture::empty();
        entity.attributes = attributes;
        entity.type_id = 7;
        entity.property_value = 3;
        entity.x = -1234;
        entity.y = 5678;
        push_i32(&mut entity.fields, 11);
        entity.fields.push(2);
        push_i32(&mut entity.fields, 512);
        push_i32(&mut entity.fields, 90);
        entity.fields.push(4);
        entity.fields.push(5);
        entity.fields.push(6);
        entity.fields.push(7);
        push_i32(&mut entity.fields, 8);
        entity.fields.push(9);
        entity.fields.push(1);
        for value in [10, 20, 30, 40] {
            push_i32(&mut entity.values, value);
        }

        let mut fixture = SceneFixture::minimal();
        fixture.entities.push(entity);
        let scene = Scene::from_bytes(&fixture.build()).unwrap();
        let entity = &scene.entities[0];
        assert_eq!(entity.attributes, attributes);
        assert_eq!(entity.type_id, 7);
        assert_eq!(entity.property_value, 3);
        assert_eq!((entity.x, entity.y), (-1234, 5678));
        assert_eq!(entity.state, Some(11));
        assert_eq!(entity.direction, Some(2));
        assert_eq!(entity.scale, Some(512));
        assert_eq!(entity.rotation, Some(90));
        assert_eq!(entity.draw_order, Some(4));
        assert_eq!(entity.priority, Some(5));
        assert_eq!(entity.alpha, Some(6));
        assert_eq!(entity.animation, Some(7));
        assert_eq!(entity.animation_speed, Some(8));
        assert_eq!(entity.frame, Some(9));
        assert_eq!(entity.ink_effect, Some(1));
        assert_eq!(entity.values, [10, 20, 30, 40]);
        assert_eq!(entity.value(0), Some(10));
        assert_eq!(entity.value(3), Some(40));
        assert_eq!(entity.value(4), None);
        // 4+1+4+4+1+1+1+1+4+1+1 optional-field bytes plus 4 * i32 values.
        assert_eq!(entity.raw_values_len, 39);
    }

    #[test]
    fn parses_entity_with_sparse_values() {
        let mut entity = EntityFixture::empty();
        entity.attributes = ENTITY_ATTRIB_VALUES[2];
        push_i32(&mut entity.values, 99);
        let mut fixture = SceneFixture::minimal();
        fixture.entities.push(entity);
        let scene = Scene::from_bytes(&fixture.build()).unwrap();
        let entity = &scene.entities[0];
        assert_eq!(entity.values, [99]);
        assert_eq!(entity.value(0), None);
        assert_eq!(entity.value(1), None);
        assert_eq!(entity.value(2), Some(99));
        assert_eq!(entity.value(3), None);
    }

    #[test]
    fn parses_maximum_layout_and_title() {
        let mut fixture = SceneFixture::minimal();
        fixture.title = "T".repeat(255);
        fixture.width = 255;
        fixture.height = 255;
        fixture.layout = vec![0xABCD; 255 * 255];
        let scene = Scene::from_bytes(&fixture.build()).unwrap();
        assert_eq!(scene.layout.len(), 255 * 255);
        assert_eq!(scene.layout[0], 0xABCD);
        assert_eq!(scene.title.len(), 255);
    }

    #[test]
    fn rejects_trailing_bytes_unless_allowed() {
        let mut bytes = SceneFixture::minimal().build();
        bytes.push(0xAA);
        assert!(matches!(
            Scene::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
        let scene = Scene::from_bytes_allow_trailing(&bytes).unwrap();
        assert_eq!(scene.layout, [0]);
    }

    #[test]
    fn truncated_inputs_error_cleanly() {
        let mut fixture = SceneFixture::minimal();
        let mut entity = EntityFixture::empty();
        entity.attributes = ENTITY_ATTRIB_STATE | ENTITY_ATTRIB_VALUES[0];
        push_i32(&mut entity.fields, -1);
        push_i32(&mut entity.values, 77);
        fixture.entities.push(entity);
        fixture.width = 2;
        fixture.height = 2;
        fixture.layout = vec![1, 2, 3, 4];
        let bytes = fixture.build();

        for cut in 0..bytes.len() {
            assert!(
                Scene::from_bytes(&bytes[..cut]).is_err(),
                "prefix of {cut} bytes unexpectedly parsed"
            );
        }
        assert!(Scene::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn rejects_invalid_utf8_title() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[2, 0xC3, 0x28]);
        bytes.extend_from_slice(&[0; ACTIVE_LAYER_COUNT]);
        bytes.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0]);
        assert!(matches!(
            Scene::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn loads_from_source() {
        let mut source = MemorySource::new();
        source.insert(
            "Data/Stages/Zone01/Act1.bin",
            SceneFixture::minimal().build(),
        );
        let scene = Scene::load("Data/Stages/Zone01", "1", &source).unwrap();
        assert_eq!(scene.title, "TEST");
        let scene = Scene::load("Data/Stages/Zone01/", "1", &source).unwrap();
        assert_eq!(scene.title, "TEST");
        assert!(matches!(
            Scene::load("Data/Stages/Missing", "1", &source),
            Err(FormatError::Io(_))
        ));
        assert_eq!(Scene::path("", "B"), "ActB.bin");
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0x1234_5678u32;
        for length in 0..512usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = Scene::from_bytes(&bytes);
            let _ = Scene::from_bytes_allow_trailing(&bytes);
        }
    }
}
