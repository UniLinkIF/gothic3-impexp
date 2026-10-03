//! A static mesh as plain arrays: positions, normals and uvs per vertex, a triangle list, and the index range of
//! each material.

#[derive(Debug, Clone, Default)]
pub struct MeshGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Triangle list, absolute indices into `positions`.
    pub indices: Vec<u32>,
    pub submeshes: Vec<SubRange>,
}

#[derive(Debug, Clone)]
pub struct SubRange { pub material: String, pub first_index: u32, pub index_count: u32 }
