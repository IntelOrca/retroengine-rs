//! Legacy v4 3D scene state: matrices, vertex/face buffers, projection and fog.
//!
//! This is the frozen M9 interface for the `scene3D.*` script variables and the matrix/
//! transform ops. [`Scene3DState::new`] is real (legacy projection defaults); every other
//! body is an inert placeholder that M9a fills from
//! `Graphics/Legacy/v4/Scene3DLegacyv4.cpp:11-314`.

/// `LEGACY_v4_VERTEXBUFFER_SIZE`.
pub const VERTEX_BUFFER_SIZE: usize = 0x1000;
/// `LEGACY_v4_FACEBUFFER_SIZE`.
pub const FACE_BUFFER_SIZE: usize = 0x400;

/// `MAT_WORLD`: the world matrix index (`MatrixTypes`).
pub const MAT_WORLD: i32 = 0;
/// `MAT_VIEW`: the view matrix index (`MatrixTypes`).
pub const MAT_VIEW: i32 = 1;
/// `MAT_TEMP`: the scratch matrix index (`MatrixTypes`).
pub const MAT_TEMP: i32 = 2;

/// `FACE_FLAG_TEXTURED_3D` (`Legacy::FaceFlags`).
pub const FACE_FLAG_TEXTURED_3D: i32 = 0;
/// `FACE_FLAG_TEXTURED_2D` (`Legacy::FaceFlags`).
pub const FACE_FLAG_TEXTURED_2D: i32 = 1;
/// `FACE_FLAG_COLORED_3D` (`Legacy::FaceFlags`).
pub const FACE_FLAG_COLORED_3D: i32 = 2;
/// `FACE_FLAG_COLORED_2D` (`Legacy::FaceFlags`).
pub const FACE_FLAG_COLORED_2D: i32 = 3;
/// `FACE_FLAG_FADED` (`Legacy::FaceFlags`).
pub const FACE_FLAG_FADED: i32 = 4;
/// `FACE_FLAG_TEXTURED_C` (`Legacy::FaceFlags`).
pub const FACE_FLAG_TEXTURED_C: i32 = 5;
/// `FACE_FLAG_TEXTURED_C_BLEND` (`Legacy::FaceFlags`).
pub const FACE_FLAG_TEXTURED_C_BLEND: i32 = 6;
/// `FACE_FLAG_3DSPRITE` (`Legacy::FaceFlags`).
pub const FACE_FLAG_3DSPRITE: i32 = 7;

/// A legacy 4x4 `Matrix`; values are 16.16 fixed point and multiplied with `>> 8` per product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Matrix {
    /// Row-major matrix elements (`values[row][column]`).
    pub values: [[i32; 4]; 4],
}

/// A legacy `Vertex`: world/view coordinates plus texture u/v.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Vertex {
    /// World x.
    pub x: i32,
    /// World y.
    pub y: i32,
    /// World z.
    pub z: i32,
    /// Texture u (or world offset for `TEXTURED_C`).
    pub u: i32,
    /// Texture v (or world offset for `TEXTURED_C`).
    pub v: i32,
}

/// A legacy `Face`: four vertex indices, packed colour and a `FACE_FLAG_*` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Face {
    /// First vertex index.
    pub a: i32,
    /// Second vertex index.
    pub b: i32,
    /// Third vertex index.
    pub c: i32,
    /// Fourth vertex index.
    pub d: i32,
    /// Packed ARGB colour (`Face.color`, stored as the script operand's bit pattern).
    pub color: u32,
    /// A `FACE_FLAG_*` value.
    pub flag: i32,
}

/// One entry of `drawList3D`: the face index and its sorted depth key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawListEntry3D {
    /// Index into `face_buffer`.
    pub face_id: i32,
    /// Sort key (`(d.z + c.z + b.z + a.z) >> 2`).
    pub depth: i32,
}

/// Legacy v4 `scene3D` globals; one instance lives on the engine's `EngineState` for script
/// access (and later `state_hash` coverage).
pub struct Scene3DState {
    /// `Legacy::vertexCount` (mesh cursor).
    pub vertex_count: i32,
    /// `Legacy::faceCount` (mesh cursor).
    pub face_count: i32,
    /// `Legacy::projectionX`, default `136`.
    pub projection_x: i32,
    /// `Legacy::projectionY`, default `160`.
    pub projection_y: i32,
    /// `Legacy::fogColor`.
    pub fog_color: i32,
    /// `Legacy::fogStrength`.
    pub fog_strength: i32,
    /// `MAT_WORLD`.
    pub mat_world: Matrix,
    /// `MAT_VIEW`.
    pub mat_view: Matrix,
    /// `MAT_TEMP`.
    pub mat_temp: Matrix,
    /// `v4::vertexBuffer`: transformed world vertices.
    pub vertex_buffer: Box<[Vertex; VERTEX_BUFFER_SIZE]>,
    /// `v4::vertexBufferT`: scratch/projection vertices (derived, not hashed).
    pub vertex_buffer_t: Box<[Vertex; VERTEX_BUFFER_SIZE]>,
    /// `v4::faceBuffer`.
    pub face_buffer: Box<[Face; FACE_BUFFER_SIZE]>,
    /// `v4::drawList3D` (derived scratch, not hashed).
    pub draw_list: Box<[DrawListEntry3D; FACE_BUFFER_SIZE]>,
}

impl Default for Scene3DState {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene3DState {
    /// Allocates the legacy buffers with the `projectionX = 136` / `projectionY = 160` defaults;
    /// everything else starts zeroed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            vertex_count: 0,
            face_count: 0,
            projection_x: 136,
            projection_y: 160,
            fog_color: 0,
            fog_strength: 0,
            mat_world: Matrix::default(),
            mat_view: Matrix::default(),
            mat_temp: Matrix::default(),
            vertex_buffer: Box::new([Vertex::default(); VERTEX_BUFFER_SIZE]),
            vertex_buffer_t: Box::new([Vertex::default(); VERTEX_BUFFER_SIZE]),
            face_buffer: Box::new([Face::default(); FACE_BUFFER_SIZE]),
            draw_list: Box::new([DrawListEntry3D::default(); FACE_BUFFER_SIZE]),
        }
    }

    /// `SetIdentityMatrix` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn set_identity(&mut self, which: i32) {
        // M9a: identity for MAT_WORLD/MAT_VIEW/MAT_TEMP.
        let _ = which;
    }

    /// `MatrixMultiply`: `A = A * B` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn multiply(&mut self, a: i32, b: i32) {
        // M9a: right-to-left sums with `wrapping_*` and `>> 8` per product.
        let _ = (a, b);
    }

    /// `MatrixTranslateXYZ` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn translate_xyz(&mut self, which: i32, x: i32, y: i32, z: i32) {
        // M9a.
        let _ = (which, x, y, z);
    }

    /// `MatrixScaleXYZ` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn scale_xyz(&mut self, which: i32, x: i32, y: i32, z: i32) {
        // M9a.
        let _ = (which, x, y, z);
    }

    /// `MatrixRotateX` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn rotate_x(&mut self, which: i32, angle: i32) {
        // M9a.
        let _ = (which, angle);
    }

    /// `MatrixRotateY` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn rotate_y(&mut self, which: i32, angle: i32) {
        // M9a.
        let _ = (which, angle);
    }

    /// `MatrixRotateZ` (`Scene3DLegacyv4.cpp`); M9a fills the body.
    pub fn rotate_z(&mut self, which: i32, angle: i32) {
        // M9a.
        let _ = (which, angle);
    }

    /// `MatrixRotateXYZ` (`Scene3DLegacyv4.cpp:164-192`); M9a fills the body. The implementation
    /// must int16-truncate each component before `& 0x1FF`.
    pub fn rotate_xyz(&mut self, which: i32, x: i32, y: i32, z: i32) {
        // M9a.
        let _ = (which, x, y, z);
    }

    /// `MatrixInverse` (`Scene3DLegacyv4.cpp:193-244`); M9a fills the body.
    pub fn inverse(&mut self, which: i32) {
        // M9a: double-precision port; return unchanged when `det == 0`.
        let _ = which;
    }

    /// `TransformVertexBuffer`: `matFinal = matWorld * matView` (`Scene3DLegacyv4.cpp`);
    /// M9a fills the body.
    pub fn transform_vertex_buffer(&mut self) {
        // M9a.
    }

    /// `TransformVertices` in place, leaving u/v alone (`Scene3DLegacyv4.cpp`); M9a fills the
    /// body.
    pub fn transform_vertices(&mut self, which: i32, start: i32, end: i32) {
        // M9a.
        let _ = (which, start, end);
    }

    /// `Sort3DDrawList`: stable descending bubble sort (`Scene3DLegacyv4.cpp:293-314`); M9a
    /// fills the body.
    pub fn sort_draw_list(&mut self) {
        // M9a.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_uses_legacy_projection_defaults() {
        let scene = Scene3DState::new();
        assert_eq!(scene.vertex_count, 0);
        assert_eq!(scene.face_count, 0);
        assert_eq!(scene.projection_x, 136);
        assert_eq!(scene.projection_y, 160);
        assert_eq!(scene.fog_color, 0);
        assert_eq!(scene.fog_strength, 0);
        assert_eq!(scene.mat_world, Matrix::default());
        assert_eq!(scene.vertex_buffer.len(), VERTEX_BUFFER_SIZE);
        assert_eq!(scene.face_buffer.len(), FACE_BUFFER_SIZE);
    }
}
