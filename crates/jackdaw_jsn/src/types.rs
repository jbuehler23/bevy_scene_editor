use std::collections::BTreeMap;

use bevy::prelude::*;

// Re-export geometry types so consumers see them from jackdaw_jsn
pub use jackdaw_geometry::{BrushFaceData, BrushPlane};

// ---------------------------------------------------------------------------
// Brush component
// ---------------------------------------------------------------------------

/// Canonical brush data. Serialized. Geometry derived from this.
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component, Default)]
pub struct Brush {
    pub faces: Vec<BrushFaceData>,
}

impl Brush {
    /// Create a cuboid brush from 6 axis-aligned face planes.
    pub fn cuboid(half_x: f32, half_y: f32, half_z: f32) -> Self {
        Self {
            faces: vec![
                // +X
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::X,
                        distance: half_x,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // -X
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::NEG_X,
                        distance: half_x,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // +Y
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::Y,
                        distance: half_y,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // -Y
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::NEG_Y,
                        distance: half_y,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // +Z
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::Z,
                        distance: half_z,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // -Z
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::NEG_Z,
                        distance: half_z,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
            ],
        }
    }

    /// Create a sphere brush approximated as an icosahedron (20 triangular faces).
    pub fn sphere(radius: f32) -> Self {
        let phi = (1.0 + 5.0_f32.sqrt()) / 2.0;
        let raw = [
            Vec3::new(-1.0, phi, 0.0),
            Vec3::new(1.0, phi, 0.0),
            Vec3::new(-1.0, -phi, 0.0),
            Vec3::new(1.0, -phi, 0.0),
            Vec3::new(0.0, -1.0, phi),
            Vec3::new(0.0, 1.0, phi),
            Vec3::new(0.0, -1.0, -phi),
            Vec3::new(0.0, 1.0, -phi),
            Vec3::new(phi, 0.0, -1.0),
            Vec3::new(phi, 0.0, 1.0),
            Vec3::new(-phi, 0.0, -1.0),
            Vec3::new(-phi, 0.0, 1.0),
        ];
        let verts: Vec<Vec3> = raw.iter().map(|v| v.normalize() * radius).collect();

        // 20 triangular faces (standard icosahedron topology)
        let tris: [[usize; 3]; 20] = [
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];

        let faces = tris
            .iter()
            .map(|&[a, b, c]| {
                let normal = (verts[b] - verts[a]).cross(verts[c] - verts[a]).normalize();
                let distance = normal.dot(verts[a]);
                // Ensure outward-facing
                let (normal, distance) = if distance < 0.0 {
                    (-normal, -distance)
                } else {
                    (normal, distance)
                };
                BrushFaceData {
                    plane: BrushPlane { normal, distance },
                    uv_scale: Vec2::ONE,
                    ..default()
                }
            })
            .collect();

        Self { faces }
    }
}

// ---------------------------------------------------------------------------
// CustomProperties component
// ---------------------------------------------------------------------------

#[derive(Component, Reflect, Default, Clone, Debug)]
#[reflect(Component, Default)]
pub struct CustomProperties {
    pub properties: BTreeMap<String, PropertyValue>,
}

// ---------------------------------------------------------------------------
// PropertyValue enum
// ---------------------------------------------------------------------------

#[derive(Reflect, Clone, Debug, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vec2(Vec2),
    Vec3(Vec3),
    Color(Color),
}

impl PropertyValue {
    /// Human-readable type label for display in UI.
    pub fn type_label(&self) -> &'static str {
        match self {
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::Vec2(_) => "Vec2",
            Self::Vec3(_) => "Vec3",
            Self::Color(_) => "Color",
        }
    }

    /// Create a default value for a given type name.
    pub fn default_for_type(name: &str) -> Option<Self> {
        match name {
            "Bool" => Some(Self::Bool(false)),
            "Int" => Some(Self::Int(0)),
            "Float" => Some(Self::Float(0.0)),
            "String" => Some(Self::String(String::new())),
            "Vec2" => Some(Self::Vec2(Vec2::ZERO)),
            "Vec3" => Some(Self::Vec3(Vec3::ZERO)),
            "Color" => Some(Self::Color(Color::WHITE)),
            _ => None,
        }
    }

    /// All available type names for the UI picker.
    pub fn all_type_names() -> &'static [&'static str] {
        &["Bool", "Int", "Float", "String", "Vec2", "Vec3", "Color"]
    }
}

// ---------------------------------------------------------------------------
// GltfSource component
// ---------------------------------------------------------------------------

#[derive(Component, Reflect, Clone)]
#[reflect(Component)]
pub struct GltfSource {
    pub path: String,
    pub scene_index: usize,
}
