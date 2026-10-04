//! Legacy v4 3D scene state: matrices, vertex/face buffers, projection and fog.
//!
//! This is the M9 interface for the `scene3D.*` script variables and the matrix/transform
//! ops, ported line-by-line from `Graphics/Legacy/v4/Scene3DLegacyv4.cpp:11-314`. The
//! implementation preserves upstream's quirks: right-to-left `>> 8` products in
//! `MatrixMultiply`, the unshifted `values[1][2] = sinX` in `MatrixRotateXYZ`, `MatrixRotateZ`
//! being byte-identical to `MatrixRotateY`, the f64 cofactor `MatrixInverse`, and the stable
//! descending bubble sort.

use retro_core::math::{COS_512_LOOKUP, SIN_512_LOOKUP};

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
/// access and `state_hash` coverage.
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

/// `sin512LookupTable[angle & 0x1FF]` (unshifted; callers apply upstream's `>> 1`).
fn sin_512(angle: i32) -> i32 {
    SIN_512_LOOKUP[(angle & 0x1FF) as usize]
}

/// `cos512LookupTable[angle & 0x1FF]` (unshifted; callers apply upstream's `>> 1`).
fn cos_512(angle: i32) -> i32 {
    COS_512_LOOKUP[(angle & 0x1FF) as usize]
}

/// `MatrixMultiply` core: `A * B` with each product shifted `>> 8` and summed right-to-left
/// (`[3]`, `[2]`, `[1]`, `[0]`).
fn multiply_matrices(matrix_a: &Matrix, matrix_b: &Matrix) -> Matrix {
    let mut values = [[0i32; 4]; 4];
    for (row, output_row) in values.iter_mut().enumerate() {
        for (col, output) in output_row.iter_mut().enumerate() {
            let mut value = matrix_a.values[row][3].wrapping_mul(matrix_b.values[3][col]) >> 8;
            value = value
                .wrapping_add(matrix_a.values[row][2].wrapping_mul(matrix_b.values[2][col]) >> 8);
            value = value
                .wrapping_add(matrix_a.values[row][1].wrapping_mul(matrix_b.values[1][col]) >> 8);
            value = value
                .wrapping_add(matrix_a.values[row][0].wrapping_mul(matrix_b.values[0][col]) >> 8);
            *output = value;
        }
    }
    Matrix { values }
}

/// `vertexBufferT[index].z`, treating out-of-range indices as upstream's unreadable memory.
fn vertex_z(buffer: &[Vertex; VERTEX_BUFFER_SIZE], index: i32) -> i32 {
    usize::try_from(index)
        .ok()
        .and_then(|index| buffer.get(index))
        .map_or(0, |vertex| vertex.z)
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

    /// Returns the matrix selected by `which`, if it is a `MAT_*` index.
    fn matrix(&self, which: i32) -> Option<&Matrix> {
        match which {
            MAT_WORLD => Some(&self.mat_world),
            MAT_VIEW => Some(&self.mat_view),
            MAT_TEMP => Some(&self.mat_temp),
            _ => None,
        }
    }

    /// Returns the mutable matrix selected by `which`, if it is a `MAT_*` index.
    fn matrix_mut(&mut self, which: i32) -> Option<&mut Matrix> {
        match which {
            MAT_WORLD => Some(&mut self.mat_world),
            MAT_VIEW => Some(&mut self.mat_view),
            MAT_TEMP => Some(&mut self.mat_temp),
            _ => None,
        }
    }

    /// Overwrites the selected matrix with `matrix`; invalid indices are ignored like upstream's
    /// switch without a default.
    fn store(&mut self, which: i32, matrix: Matrix) {
        if let Some(target) = self.matrix_mut(which) {
            *target = matrix;
        }
    }

    /// `SetIdentityMatrix` (`Scene3DLegacyv4.cpp:11-33`).
    pub fn set_identity(&mut self, which: i32) {
        let matrix = Matrix {
            values: [
                [0x100, 0, 0, 0],
                [0, 0x100, 0, 0],
                [0, 0, 0x100, 0],
                [0, 0, 0, 0x100],
            ],
        };
        self.store(which, matrix);
    }

    /// `MatrixMultiply`: `A = A * B` (`Scene3DLegacyv4.cpp:34-47`). The output is computed into a
    /// local first, so `A` and `B` may alias.
    pub fn multiply(&mut self, a: i32, b: i32) {
        let (Some(matrix_a), Some(matrix_b)) = (self.matrix(a).copied(), self.matrix(b).copied())
        else {
            return;
        };
        let output = multiply_matrices(&matrix_a, &matrix_b);
        self.store(a, output);
    }

    /// `MatrixTranslateXYZ` (`Scene3DLegacyv4.cpp:48-68`).
    pub fn translate_xyz(&mut self, which: i32, x: i32, y: i32, z: i32) {
        let matrix = Matrix {
            values: [
                [0x100, 0, 0, 0],
                [0, 0x100, 0, 0],
                [0, 0, 0x100, 0],
                [x, y, z, 0x100],
            ],
        };
        self.store(which, matrix);
    }

    /// `MatrixScaleXYZ` (`Scene3DLegacyv4.cpp:69-90`).
    pub fn scale_xyz(&mut self, which: i32, x: i32, y: i32, z: i32) {
        let matrix = Matrix {
            values: [[x, 0, 0, 0], [0, y, 0, 0], [0, 0, z, 0], [0, 0, 0, 0x100]],
        };
        self.store(which, matrix);
    }

    /// `MatrixRotateX` (`Scene3DLegacyv4.cpp:91-113`).
    pub fn rotate_x(&mut self, which: i32, angle: i32) {
        let sine = sin_512(angle) >> 1;
        let cosine = cos_512(angle) >> 1;
        let matrix = Matrix {
            values: [
                [0x100, 0, 0, 0],
                [0, cosine, sine, 0],
                [0, sine.wrapping_neg(), cosine, 0],
                [0, 0, 0, 0x100],
            ],
        };
        self.store(which, matrix);
    }

    /// `MatrixRotateY` (`Scene3DLegacyv4.cpp:114-136`).
    pub fn rotate_y(&mut self, which: i32, angle: i32) {
        let matrix = rotation_yz(angle);
        self.store(which, matrix);
    }

    /// `MatrixRotateZ` (`Scene3DLegacyv4.cpp:137-162`). Upstream's body is a copy of
    /// `MatrixRotateY` (same matrix), preserved here.
    pub fn rotate_z(&mut self, which: i32, angle: i32) {
        let matrix = rotation_yz(angle);
        self.store(which, matrix);
    }

    /// `MatrixRotateXYZ` (`Scene3DLegacyv4.cpp:163-193`). The angles arrive as C `int16`
    /// parameters and are truncated before `& 0x1FF`; `values[1][2] = sinX` stays unshifted.
    pub fn rotate_xyz(&mut self, which: i32, x: i32, y: i32, z: i32) {
        let x = (x as i16) as i32;
        let y = (y as i16) as i32;
        let z = (z as i16) as i32;
        let sin_x = sin_512(x) >> 1;
        let cos_x = cos_512(x) >> 1;
        let sin_y = sin_512(y) >> 1;
        let cos_y = cos_512(y) >> 1;
        let sin_z = sin_512(z) >> 1;
        let cos_z = cos_512(z) >> 1;

        let matrix = Matrix {
            values: [
                [
                    (cos_z.wrapping_mul(cos_y) >> 8)
                        .wrapping_add(sin_z.wrapping_mul(sin_y.wrapping_mul(sin_x) >> 8) >> 8),
                    (sin_z.wrapping_mul(cos_y) >> 8)
                        .wrapping_sub(cos_z.wrapping_mul(sin_y.wrapping_mul(sin_x) >> 8) >> 8),
                    sin_y.wrapping_mul(cos_x) >> 8,
                    0,
                ],
                [
                    sin_z.wrapping_mul(cos_x.wrapping_neg()) >> 8,
                    cos_z.wrapping_mul(cos_x) >> 8,
                    sin_x,
                    0,
                ],
                [
                    (sin_z.wrapping_mul(cos_y.wrapping_mul(sin_x) >> 8) >> 8)
                        .wrapping_sub(cos_z.wrapping_mul(sin_y) >> 8),
                    (sin_z.wrapping_mul(sin_y.wrapping_neg()) >> 8)
                        .wrapping_sub(cos_z.wrapping_mul(cos_y.wrapping_mul(sin_x) >> 8) >> 8),
                    cos_y.wrapping_mul(cos_x) >> 8,
                    0,
                ],
                [0, 0, 0, 0x100],
            ],
        };
        self.store(which, matrix);
    }

    /// `MatrixInverse` (`Scene3DLegacyv4.cpp:194-245`): the f64 cofactor inverse with
    /// `m[y][x] / 256.0`, left unchanged when `det == 0`.
    pub fn inverse(&mut self, which: i32) {
        let Some(matrix) = self.matrix_mut(which) else {
            return;
        };
        let mut m = [0.0f64; 16];
        for (y, row) in matrix.values.iter().enumerate() {
            for (x, value) in row.iter().enumerate() {
                m[(y << 2) + x] = f64::from(*value) / 256.0;
            }
        }

        let mut inv = [0.0f64; 16];
        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
            + m[9] * m[7] * m[14]
            + m[13] * m[6] * m[11]
            - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
            - m[8] * m[7] * m[14]
            - m[12] * m[6] * m[11]
            + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
            + m[8] * m[7] * m[13]
            + m[12] * m[5] * m[11]
            - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
            - m[8] * m[6] * m[13]
            - m[12] * m[5] * m[10]
            + m[12] * m[6] * m[9];
        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
            - m[9] * m[3] * m[14]
            - m[13] * m[2] * m[11]
            + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
            + m[8] * m[3] * m[14]
            + m[12] * m[2] * m[11]
            - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
            - m[8] * m[3] * m[13]
            - m[12] * m[1] * m[11]
            + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
            + m[8] * m[2] * m[13]
            + m[12] * m[1] * m[10]
            - m[12] * m[2] * m[9];
        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
            + m[5] * m[3] * m[14]
            + m[13] * m[2] * m[7]
            - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
            - m[4] * m[3] * m[14]
            - m[12] * m[2] * m[7]
            + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
            + m[4] * m[3] * m[13]
            + m[12] * m[1] * m[7]
            - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
            - m[4] * m[2] * m[13]
            - m[12] * m[1] * m[6]
            + m[12] * m[2] * m[5];
        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
            - m[5] * m[3] * m[10]
            - m[9] * m[2] * m[7]
            + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
            + m[4] * m[3] * m[10]
            + m[8] * m[2] * m[7]
            - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
            - m[4] * m[3] * m[9]
            - m[8] * m[1] * m[7]
            + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
            + m[4] * m[2] * m[9]
            + m[8] * m[1] * m[6]
            - m[8] * m[2] * m[5];

        let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        if det == 0.0 {
            return;
        }
        let det = 1.0 / det;
        for (index, value) in inv.iter_mut().enumerate() {
            let [y, x] = [index / 4, index % 4];
            matrix.values[y][x] = ((*value * det) * 256.0) as i32;
        }
    }

    /// `TransformVertexBuffer` (`Scene3DLegacyv4.cpp:246-282`): builds `matFinal = matWorld *
    /// matView` and projects `vertexBuffer[0..vertexCount]` into `vertexBufferT`, leaving u/v
    /// untouched.
    pub fn transform_vertex_buffer(&mut self) {
        let mat_final = multiply_matrices(&self.mat_world, &self.mat_view);
        let count = usize::try_from(self.vertex_count)
            .unwrap_or(0)
            .min(VERTEX_BUFFER_SIZE);
        for index in 0..count {
            let vertex = self.vertex_buffer[index];
            let target = &mut self.vertex_buffer_t[index];
            target.x = (vertex.x.wrapping_mul(mat_final.values[0][0]) >> 8)
                .wrapping_add(vertex.y.wrapping_mul(mat_final.values[1][0]) >> 8)
                .wrapping_add(vertex.z.wrapping_mul(mat_final.values[2][0]) >> 8)
                .wrapping_add(mat_final.values[3][0]);
            target.y = (vertex.x.wrapping_mul(mat_final.values[0][1]) >> 8)
                .wrapping_add(vertex.y.wrapping_mul(mat_final.values[1][1]) >> 8)
                .wrapping_add(vertex.z.wrapping_mul(mat_final.values[2][1]) >> 8)
                .wrapping_add(mat_final.values[3][1]);
            target.z = (vertex.x.wrapping_mul(mat_final.values[0][2]) >> 8)
                .wrapping_add(vertex.y.wrapping_mul(mat_final.values[1][2]) >> 8)
                .wrapping_add(vertex.z.wrapping_mul(mat_final.values[2][2]) >> 8)
                .wrapping_add(mat_final.values[3][2]);
        }
    }

    /// `TransformVertices` (`Scene3DLegacyv4.cpp:283-300`): in-place transform of
    /// `vertexBuffer[start..end]`, leaving each vertex's u/v alone.
    pub fn transform_vertices(&mut self, which: i32, start: i32, end: i32) {
        let Some(matrix) = self.matrix(which).copied() else {
            return;
        };
        let limit = VERTEX_BUFFER_SIZE as i32;
        let start = start.clamp(0, limit);
        let end = end.clamp(0, limit);
        for index in start..end {
            let index = index as usize;
            let vertex = self.vertex_buffer[index];
            let target = &mut self.vertex_buffer[index];
            target.x = (vertex.x.wrapping_mul(matrix.values[0][0]) >> 8)
                .wrapping_add(vertex.y.wrapping_mul(matrix.values[1][0]) >> 8)
                .wrapping_add(vertex.z.wrapping_mul(matrix.values[2][0]) >> 8)
                .wrapping_add(matrix.values[3][0]);
            target.y = (vertex.x.wrapping_mul(matrix.values[0][1]) >> 8)
                .wrapping_add(vertex.y.wrapping_mul(matrix.values[1][1]) >> 8)
                .wrapping_add(vertex.z.wrapping_mul(matrix.values[2][1]) >> 8)
                .wrapping_add(matrix.values[3][1]);
            target.z = (vertex.x.wrapping_mul(matrix.values[0][2]) >> 8)
                .wrapping_add(vertex.y.wrapping_mul(matrix.values[1][2]) >> 8)
                .wrapping_add(vertex.z.wrapping_mul(matrix.values[2][2]) >> 8)
                .wrapping_add(matrix.values[3][2]);
        }
    }

    /// `Sort3DDrawList` (`Scene3DLegacyv4.cpp:301-325`): fills `drawList3D[0..faceCount]` and
    /// stable-bubble-sorts it by descending `(d.z + c.z + b.z + a.z) >> 2`.
    pub fn sort_draw_list(&mut self) {
        let count = usize::try_from(self.face_count)
            .unwrap_or(0)
            .min(FACE_BUFFER_SIZE);
        for (index, entry) in self.draw_list.iter_mut().take(count).enumerate() {
            let face = self.face_buffer[index];
            let depth = vertex_z(&self.vertex_buffer_t, face.d)
                .wrapping_add(vertex_z(&self.vertex_buffer_t, face.c))
                .wrapping_add(vertex_z(&self.vertex_buffer_t, face.b))
                .wrapping_add(vertex_z(&self.vertex_buffer_t, face.a))
                >> 2;
            *entry = DrawListEntry3D {
                face_id: index as i32,
                depth,
            };
        }
        for i in 0..count {
            for j in (i + 1..count).rev() {
                if self.draw_list[j].depth > self.draw_list[j - 1].depth {
                    self.draw_list.swap(j, j - 1);
                }
            }
        }
    }
}

/// The `MatrixRotateY` body, shared by `MatrixRotateZ` because upstream's Z rotation is a
/// byte-for-byte copy (both rotate the x/z axes).
fn rotation_yz(angle: i32) -> Matrix {
    let sine = sin_512(angle) >> 1;
    let cosine = cos_512(angle) >> 1;
    Matrix {
        values: [
            [cosine, 0, sine, 0],
            [0, 0x100, 0, 0],
            [sine.wrapping_neg(), 0, cosine, 0],
            [0, 0, 0, 0x100],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Matrix {
        Matrix {
            values: [
                [0x100, 0, 0, 0],
                [0, 0x100, 0, 0],
                [0, 0, 0x100, 0],
                [0, 0, 0, 0x100],
            ],
        }
    }

    fn assert_identity(matrix: &Matrix) {
        assert_eq!(*matrix, identity());
    }

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

    #[test]
    fn set_identity_writes_each_matrix_and_ignores_invalid_indices() {
        let mut scene = Scene3DState::new();
        scene.mat_world.values = [[7; 4]; 4];
        scene.mat_view.values = [[7; 4]; 4];
        scene.mat_temp.values = [[7; 4]; 4];
        for which in [MAT_WORLD, MAT_VIEW, MAT_TEMP] {
            scene.set_identity(which);
        }
        assert_identity(&scene.mat_world);
        assert_identity(&scene.mat_view);
        assert_identity(&scene.mat_temp);

        scene.mat_world.values[0][0] = 9;
        scene.set_identity(99);
        assert_eq!(scene.mat_world.values[0][0], 9);
    }

    #[test]
    fn translate_and_scale_write_their_rows() {
        let mut scene = Scene3DState::new();
        scene.translate_xyz(MAT_WORLD, 0x100, 0x200, 0x300);
        let mut expected = identity();
        expected.values[3] = [0x100, 0x200, 0x300, 0x100];
        assert_eq!(scene.mat_world, expected);

        scene.scale_xyz(MAT_TEMP, 0x180, 0x200, 0x80);
        let mut expected = identity();
        expected.values[0][0] = 0x180;
        expected.values[1][1] = 0x200;
        expected.values[2][2] = 0x80;
        assert_eq!(scene.mat_temp, expected);
    }

    #[test]
    fn rotate_x_matches_the_upstream_table() {
        let mut scene = Scene3DState::new();
        scene.rotate_x(MAT_VIEW, 0x80);

        let sine = SIN_512_LOOKUP[0x80] >> 1;
        let cosine = COS_512_LOOKUP[0x80] >> 1;
        let expected = Matrix {
            values: [
                [0x100, 0, 0, 0],
                [0, cosine, sine, 0],
                [0, -sine, cosine, 0],
                [0, 0, 0, 0x100],
            ],
        };
        assert_eq!(scene.mat_view, expected);
        assert_eq!(sine, 0x100);
        assert_eq!(cosine, 0);
    }

    #[test]
    fn rotate_y_and_rotate_z_are_the_same_upstream_matrix() {
        let mut scene = Scene3DState::new();
        scene.rotate_y(MAT_WORLD, 0x80);
        scene.rotate_z(MAT_VIEW, 0x80);
        assert_eq!(scene.mat_world, scene.mat_view);

        let sine = SIN_512_LOOKUP[0x80] >> 1;
        let cosine = COS_512_LOOKUP[0x80] >> 1;
        let expected = Matrix {
            values: [
                [cosine, 0, sine, 0],
                [0, 0x100, 0, 0],
                [-sine, 0, cosine, 0],
                [0, 0, 0, 0x100],
            ],
        };
        assert_eq!(scene.mat_world, expected);
    }

    #[test]
    fn rotate_xyz_matches_rotate_x_for_cardinal_inputs() {
        let mut scene = Scene3DState::new();
        scene.rotate_xyz(MAT_TEMP, 0x80, 0, 0);
        let mut reference = Scene3DState::new();
        reference.rotate_x(MAT_TEMP, 0x80);
        assert_eq!(scene.mat_temp, reference.mat_temp);
    }

    #[test]
    fn rotate_xyz_keeps_the_unshifted_sin_x_and_truncates_int16() {
        let mut scene = Scene3DState::new();
        scene.rotate_xyz(MAT_WORLD, 0x40, 0, 0);

        let sin_x = SIN_512_LOOKUP[0x40] >> 1;
        let cos_x = COS_512_LOOKUP[0x40] >> 1;
        assert_eq!(sin_x, 181);
        assert_eq!(
            scene.mat_world.values[1][2], sin_x,
            "values[1][2] is the unshifted sinX"
        );
        assert_eq!(scene.mat_world.values[1][1], (256 * cos_x) >> 8);
        assert_eq!(scene.mat_world.values[2][1], -((256 * sin_x) >> 8));

        // C `int16` parameters: 0x1_0080 truncates to 0x80 before `& 0x1FF`.
        let mut wrapped = Scene3DState::new();
        wrapped.rotate_xyz(MAT_WORLD, 0x1_0080, 0, 0);
        let mut plain = Scene3DState::new();
        plain.rotate_xyz(MAT_WORLD, 0x80, 0, 0);
        assert_eq!(wrapped.mat_world, plain.mat_world);

        // -1 (int16) `& 0x1FF` == 0x1FF.
        let mut negative = Scene3DState::new();
        negative.rotate_xyz(MAT_WORLD, -1, 0, 0);
        let mut last = Scene3DState::new();
        last.rotate_xyz(MAT_WORLD, 0x1FF, 0, 0);
        assert_eq!(negative.mat_world, last.mat_world);
    }

    #[test]
    fn multiply_with_identity_preserves_the_target() {
        let mut scene = Scene3DState::new();
        scene.translate_xyz(MAT_VIEW, 0x100, 0x200, 0x300);
        let translated = scene.mat_view;

        scene.set_identity(MAT_TEMP);
        scene.set_identity(MAT_WORLD);
        scene.multiply(MAT_WORLD, MAT_VIEW); // I * A == A
        assert_eq!(scene.mat_world, translated);

        scene.multiply(MAT_VIEW, MAT_TEMP); // A * I == A
        assert_eq!(scene.mat_view, translated);

        // Aliased operands: A = A * A must not read partially-written output.
        scene.multiply(MAT_VIEW, MAT_VIEW);
        let square = multiply_matrices(&translated, &translated);
        assert_eq!(scene.mat_view, square);
    }

    #[test]
    fn multiply_applies_each_product_shift_right_eight() {
        let mut scene = Scene3DState::new();
        scene.scale_xyz(MAT_TEMP, 0x200, 0x200, 0x200);
        scene.translate_xyz(MAT_WORLD, 0x100, 0x200, 0x300);
        scene.multiply(MAT_WORLD, MAT_TEMP);

        // translate * scale: the scale's diagonal multiplies the translation row.
        let mut expected = identity();
        expected.values[0][0] = 0x200;
        expected.values[1][1] = 0x200;
        expected.values[2][2] = 0x200;
        expected.values[3] = [0x200, 0x400, 0x600, 0x100];
        assert_eq!(scene.mat_world, expected);
    }

    #[test]
    fn inverse_of_translate_negates_the_translation() {
        let mut scene = Scene3DState::new();
        scene.translate_xyz(MAT_WORLD, 0x100, 0x200, 0x300);
        scene.inverse(MAT_WORLD);
        let mut expected = identity();
        expected.values[3] = [-0x100, -0x200, -0x300, 0x100];
        assert_eq!(scene.mat_world, expected);
    }

    #[test]
    fn inverse_of_rotate_x_inverts_the_rotation() {
        let mut scene = Scene3DState::new();
        scene.rotate_x(MAT_VIEW, 0x80);
        scene.inverse(MAT_VIEW);
        let mut expected = Scene3DState::new();
        expected.rotate_x(MAT_VIEW, 0x180);
        assert_eq!(scene.mat_view, expected.mat_view);
    }

    #[test]
    fn inverse_leaves_singular_matrices_unchanged() {
        let mut scene = Scene3DState::new();
        scene.scale_xyz(MAT_TEMP, 0, 0x100, 0x100);
        let before = scene.mat_temp;
        scene.inverse(MAT_TEMP);
        assert_eq!(scene.mat_temp, before);
    }

    #[test]
    fn transform_vertex_buffer_projects_with_world_times_view() {
        let mut scene = Scene3DState::new();
        scene.set_identity(MAT_WORLD);
        scene.set_identity(MAT_VIEW);
        scene.vertex_count = 2;
        scene.vertex_buffer[0] = Vertex {
            x: 0x100,
            y: 0x200,
            z: 0x300,
            u: 5,
            v: 6,
        };
        scene.transform_vertex_buffer();
        assert_eq!(
            scene.vertex_buffer_t[0],
            Vertex {
                x: 0x100,
                y: 0x200,
                z: 0x300,
                u: 0,
                v: 0,
            },
            "matFinal identity; u/v are never written by TransformVertexBuffer"
        );

        // matFinal = matWorld * matView; the view scale scales the world translation.
        scene.translate_xyz(MAT_WORLD, 0x100, 0, 0);
        scene.scale_xyz(MAT_VIEW, 0x200, 0x200, 0x200);
        scene.vertex_buffer[1] = Vertex {
            x: 0x80,
            y: 0x40,
            z: 0x20,
            u: 7,
            v: 9,
        };
        scene.transform_vertex_buffer();
        assert_eq!(
            scene.vertex_buffer_t[1],
            Vertex {
                x: 0x300,
                y: 0x80,
                z: 0x40,
                u: 0,
                v: 0,
            }
        );
    }

    #[test]
    fn transform_vertices_updates_only_the_range_and_keeps_uv() {
        let mut scene = Scene3DState::new();
        scene.scale_xyz(MAT_TEMP, 0x200, 0x200, 0x200);
        scene.vertex_buffer[0] = Vertex {
            x: 0x10,
            y: 0x20,
            z: 0x30,
            u: 1,
            v: 2,
        };
        scene.vertex_buffer[1] = Vertex {
            x: 0x80,
            y: 0x40,
            z: 0x20,
            u: 3,
            v: 4,
        };
        scene.vertex_buffer[2] = Vertex {
            x: 0x40,
            y: 0x80,
            z: 0x10,
            u: 5,
            v: 6,
        };
        scene.transform_vertices(MAT_TEMP, 1, 3);
        assert_eq!(scene.vertex_buffer[0].x, 0x10, "start is exclusive");
        assert_eq!(
            scene.vertex_buffer[1],
            Vertex {
                x: 0x100,
                y: 0x80,
                z: 0x40,
                u: 3,
                v: 4,
            }
        );
        assert_eq!(
            scene.vertex_buffer[2],
            Vertex {
                x: 0x80,
                y: 0x100,
                z: 0x20,
                u: 5,
                v: 6,
            }
        );

        scene.transform_vertices(MAT_TEMP, 2, 1);
        assert_eq!(scene.vertex_buffer[2].x, 0x80, "end < start is a no-op");

        scene.transform_vertices(99, 0, 3);
        assert_eq!(scene.vertex_buffer[1].x, 0x100, "invalid matrix is ignored");
    }

    #[test]
    fn sort_draw_list_is_stable_descending_with_arithmetic_shift() {
        let mut scene = Scene3DState::new();
        scene.face_count = 4;
        scene.vertex_buffer_t[0].z = 0x30;
        scene.vertex_buffer_t[1].z = 0x10;
        scene.vertex_buffer_t[2].z = 0x30;
        scene.vertex_buffer_t[3].z = -1;
        for index in 0..4i32 {
            scene.face_buffer[index as usize] = Face {
                a: index,
                b: index,
                c: index,
                d: index,
                ..Face::default()
            };
        }
        scene.sort_draw_list();

        // Depths: face 0 = 0x30, face 1 = 0x10, face 2 = 0x30, face 3 = (-1) >> 2 = -1.
        // Descending with ties keeping face order: 0, 2, 1, 3.
        let order: Vec<i32> = scene.draw_list[..4]
            .iter()
            .map(|entry| entry.face_id)
            .collect();
        assert_eq!(order, vec![0, 2, 1, 3]);
        assert_eq!(scene.draw_list[0].depth, 0x30);
        assert_eq!(scene.draw_list[1].depth, 0x30);
        assert_eq!(scene.draw_list[2].depth, 0x10);
        assert_eq!(scene.draw_list[3].depth, -1, "arithmetic shift, not / 4");
    }
}
