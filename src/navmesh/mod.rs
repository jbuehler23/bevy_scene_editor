mod brp_client;
mod build;
mod save_load;
pub mod toolbar;
mod visualization;

use bevy::prelude::*;
use bevy_rerecast::{prelude::*, rerecast::TriMesh};

pub use toolbar::NavmeshToolbar;

pub struct NavmeshPlugin;

impl Plugin for NavmeshPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(
            NavmeshPlugins::default()
                .build()
                .disable::<bevy_rerecast::debug::NavmeshDebugPlugin>(),
        );
        app.set_navmesh_backend(editor_backend);
        app.init_resource::<NavmeshObstacles>()
            .init_resource::<NavmeshHandleRes>()
            .init_resource::<NavmeshState>();
        app.add_plugins((
            build::plugin,
            brp_client::plugin,
            save_load::plugin,
            toolbar::plugin,
            visualization::plugin,
        ));
    }
}

fn editor_backend(_: In<NavmeshSettings>, obstacles: Res<NavmeshObstacles>) -> TriMesh {
    obstacles.0.clone()
}

#[derive(Resource, Deref, DerefMut, Default)]
pub struct NavmeshObstacles(pub TriMesh);

#[derive(Resource, Default, Deref, DerefMut)]
pub struct NavmeshHandleRes(pub Handle<Navmesh>);

#[derive(Resource, Default)]
pub struct NavmeshState {
    pub status: NavmeshStatus,
}

#[derive(Default, Clone, Debug)]
pub enum NavmeshStatus {
    #[default]
    Idle,
    FetchingScene,
    Building,
    Ready,
    Error(String),
}

impl std::fmt::Display for NavmeshStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Navmesh: Idle"),
            Self::FetchingScene => write!(f, "Navmesh: Fetching scene..."),
            Self::Building => write!(f, "Navmesh: Building..."),
            Self::Ready => write!(f, "Navmesh: Ready"),
            Self::Error(e) => write!(f, "Navmesh: Error - {e}"),
        }
    }
}

pub fn spawn_navmesh_entity(commands: &mut Commands) {
    commands.spawn((
        Name::new("Navmesh"),
        Transform::from_scale(Vec3::splat(10.0)),
        jackdaw_jsn::NavmeshRegion::default(),
    ));
}
