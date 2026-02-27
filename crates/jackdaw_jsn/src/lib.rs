pub mod format;
mod loader;
mod mesh_rebuild;
pub mod types;

use bevy::prelude::*;

// Re-export core types for consumer convenience
pub use types::{Brush, BrushFaceData, BrushPlane, CustomProperties, GltfSource, PropertyValue};

// Re-export geometry crate
pub use jackdaw_geometry;

pub use format::JsnScene;
pub use loader::JsnAssetLoader;

pub struct JsnPlugin;

impl Plugin for JsnPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Brush>()
            .register_type::<BrushFaceData>()
            .register_type::<BrushPlane>()
            .register_type::<CustomProperties>()
            .register_type::<PropertyValue>()
            .register_type::<GltfSource>()
            .init_asset_loader::<JsnAssetLoader>()
            .add_systems(Update, mesh_rebuild::rebuild_brush_meshes);
    }
}
