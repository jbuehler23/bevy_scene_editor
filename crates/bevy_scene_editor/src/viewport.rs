use bevy::prelude::*;
use bevy::picking::pointer::PointerInteraction;
use crate::state::{EditorEntity, EditorState};

#[derive(Component)]
pub struct EditorCamera;

#[derive(Resource)]
pub struct OrbitState {
    pub focus: Vec3,
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for OrbitState {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            radius: 10.0,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: std::f32::consts::FRAC_PI_6,
        }
    }
}

pub fn spawn_editor_camera(mut commands: Commands) {
    commands.init_resource::<OrbitState>();

    let orbit = OrbitState::default();
    let eye = orbit_eye(&orbit);

    commands.spawn((
        EditorEntity,
        EditorCamera,
        Camera3d::default(),
        Transform::from_translation(eye).looking_at(orbit.focus, Vec3::Y),
    ));
}

fn orbit_eye(orbit: &OrbitState) -> Vec3 {
    let x = orbit.radius * orbit.pitch.cos() * orbit.yaw.sin();
    let y = orbit.radius * orbit.pitch.sin();
    let z = orbit.radius * orbit.pitch.cos() * orbit.yaw.cos();
    orbit.focus + Vec3::new(x, y, z)
}

pub fn orbit_camera_system(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut scroll: MessageReader<bevy::input::mouse::MouseWheel>,
    mut orbit: ResMut<OrbitState>,
    mut camera: Query<&mut Transform, With<EditorCamera>>,
) {
    let mut delta = Vec2::ZERO;
    for event in mouse_motion.read() {
        delta += event.delta;
    }

    let mut scroll_delta = 0.0;
    for event in scroll.read() {
        scroll_delta += event.y;
    }

    let mut changed = false;

    // Right-drag: orbit
    if mouse_button.pressed(MouseButton::Right) && delta != Vec2::ZERO {
        orbit.yaw -= delta.x * 0.005;
        orbit.pitch += delta.y * 0.005;
        orbit.pitch = orbit.pitch.clamp(-1.4, 1.4);
        changed = true;
    }

    // Middle-drag: pan
    if mouse_button.pressed(MouseButton::Middle) && delta != Vec2::ZERO {
        let Ok(transform) = camera.single() else {
            return;
        };
        let right = transform.right();
        let up = transform.up();
        let pan_speed = orbit.radius * 0.002;
        orbit.focus -= right * delta.x * pan_speed;
        orbit.focus += up * delta.y * pan_speed;
        changed = true;
    }

    // Scroll: zoom
    if scroll_delta != 0.0 {
        orbit.radius *= 1.0 - scroll_delta * 0.1;
        orbit.radius = orbit.radius.clamp(0.5, 100.0);
        changed = true;
    }

    if changed {
        if let Ok(mut transform) = camera.single_mut() {
            let eye = orbit_eye(&orbit);
            *transform = Transform::from_translation(eye).looking_at(orbit.focus, Vec3::Y);
        }
    }
}

pub fn picking_system(
    mut editor_state: ResMut<EditorState>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    pointers: Query<&PointerInteraction>,
    editor_entities: Query<(), With<EditorEntity>>,
) {
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    for interaction in &pointers {
        for (entity, _) in interaction.iter() {
            // Don't select editor UI entities
            if editor_entities.contains(*entity) {
                continue;
            }
            editor_state.selected_entity = Some(*entity);
            return;
        }
    }
}
