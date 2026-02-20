use bevy::prelude::*;

pub struct NumericInputPlugin;

impl Plugin for NumericInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NumericDragState>()
            .add_systems(Update, (update_numeric_drag, update_numeric_display));
    }
}

#[derive(Component)]
pub struct NumericInput {
    pub value: f64,
    pub step: f64,
    pub precision: usize,
}

impl NumericInput {
    pub fn new(value: f64) -> Self {
        Self {
            value,
            step: 0.01,
            precision: 3,
        }
    }

    pub fn formatted(&self) -> String {
        format!("{:.prec$}", self.value, prec = self.precision)
    }
}

#[derive(Component)]
pub struct NumericInputDisplay;

#[derive(EntityEvent)]
pub struct NumericValueChanged {
    pub entity: Entity,
    pub value: f64,
}

#[derive(Resource, Default)]
pub struct NumericDragState {
    pub active: Option<ActiveDrag>,
}

pub struct ActiveDrag {
    pub entity: Entity,
    pub last_screen_x: f32,
}

/// Attach this as a per-entity observer on numeric input entities.
pub fn start_numeric_drag(
    down: On<Pointer<Press>>,
    mut state: ResMut<NumericDragState>,
    windows: Query<&Window>,
) {
    let entity = down.event_target();
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(pos) = window.cursor_position() else {
        return;
    };
    state.active = Some(ActiveDrag {
        entity,
        last_screen_x: pos.x,
    });
}

fn update_numeric_drag(
    mut state: ResMut<NumericDragState>,
    mut query: Query<&mut NumericInput>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    let Some(active) = &mut state.active else {
        return;
    };

    if !mouse.pressed(MouseButton::Left) {
        state.active = None;
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    let delta_x = cursor_pos.x - active.last_screen_x;
    active.last_screen_x = cursor_pos.x;

    if delta_x.abs() < 0.1 {
        return;
    }

    let Ok(mut input) = query.get_mut(active.entity) else {
        state.active = None;
        return;
    };

    let mut sensitivity = input.step;
    if keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        sensitivity *= 0.1;
    }
    if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        sensitivity *= 10.0;
    }

    input.value += delta_x as f64 * sensitivity;

    let entity = active.entity;
    let value = input.value;
    commands.trigger(NumericValueChanged { entity, value });
}

fn update_numeric_display(
    inputs: Query<(&NumericInput, &Children), Changed<NumericInput>>,
    mut displays: Query<&mut Text, With<NumericInputDisplay>>,
) {
    for (input, children) in &inputs {
        for child in children.iter() {
            if let Ok(mut text) = displays.get_mut(child) {
                text.0 = input.formatted();
            }
        }
    }
}
