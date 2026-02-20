use bevy::prelude::*;

/// System set containing the context menu close systems.
/// Order your context menu openers `.after(ContextMenuCloseSet)` to avoid
/// the close system immediately despawning a just-created menu.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContextMenuCloseSet;

pub struct ContextMenuPlugin;

impl Plugin for ContextMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (close_context_menu_on_click, close_context_menu_on_escape)
                .in_set(ContextMenuCloseSet),
        );
    }
}

/// Marker component for the context menu container.
#[derive(Component)]
pub struct ContextMenu;

/// Individual menu item with an action identifier.
#[derive(Component)]
pub struct ContextMenuItem {
    pub action: String,
    /// The entity that the context menu was opened for (stored at spawn time).
    pub target_entity: Option<Entity>,
}

/// Event fired when a context menu item is clicked.
#[derive(Event, Debug, Clone)]
pub struct ContextMenuAction {
    pub action: String,
    /// The entity that the context menu was opened for (e.g., the hierarchy entity).
    pub target_entity: Option<Entity>,
}

/// Resource tracking the context menu's target entity.
#[derive(Resource, Default)]
pub struct ContextMenuState {
    pub target_entity: Option<Entity>,
    pub menu_entity: Option<Entity>,
}

/// Close context menu when clicking outside it.
fn close_context_menu_on_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut state: Option<ResMut<ContextMenuState>>,
) {
    if !mouse.just_pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let Some(ref mut state) = state else {
        return;
    };
    if let Some(menu) = state.menu_entity.take() {
        commands.entity(menu).despawn();
    }
    state.target_entity = None;
}

/// Close on Escape key.
fn close_context_menu_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut state: Option<ResMut<ContextMenuState>>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(ref mut state) = state else {
        return;
    };
    if let Some(menu) = state.menu_entity.take() {
        commands.entity(menu).despawn();
    }
    state.target_entity = None;
}
