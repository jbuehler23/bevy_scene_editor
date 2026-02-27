use bevy::prelude::*;
use jackdaw_widgets::context_menu::{ContextMenuAction, ContextMenuItem};

use crate::button::{button, ButtonClickEvent, ButtonProps, ButtonVariant};
use crate::tokens;

pub fn plugin(app: &mut App) {
    app.add_observer(on_context_menu_item_click);
}

fn on_context_menu_item_click(
    event: On<ButtonClickEvent>,
    items: Query<&ContextMenuItem>,
    mut commands: Commands,
) {
    let Ok(item) = items.get(event.entity) else {
        return;
    };
    commands.trigger(ContextMenuAction {
        action: item.action.clone(),
        target_entity: item.target_entity,
    });
}

/// Spawn a context menu at the given position with the given items.
/// Each item is (action_id, label).
pub fn spawn_context_menu(
    commands: &mut Commands,
    position: Vec2,
    target_entity: Option<Entity>,
    items: &[(&str, &str)],
) -> Entity {
    let menu = commands
        .spawn((
            jackdaw_widgets::context_menu::ContextMenu,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x),
                top: Val::Px(position.y),
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(160.0),
                padding: UiRect::axes(Val::Px(tokens::SPACING_XS), Val::Px(tokens::SPACING_SM)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(tokens::BORDER_RADIUS_MD)),
                ..Default::default()
            },
            BackgroundColor(tokens::MENU_BG),
            BorderColor::all(tokens::BORDER_SUBTLE),
            ZIndex(1000),
        ))
        .id();

    for &(action, label) in items {
        commands.entity(menu).with_child((
            ContextMenuItem {
                action: action.to_string(),
                target_entity,
            },
            button(
                ButtonProps::new(label)
                    .with_variant(ButtonVariant::Ghost)
                    .align_left(),
            ),
        ));
    }

    menu
}
