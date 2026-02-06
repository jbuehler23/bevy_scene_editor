use bevy::{prelude::*, ui_widgets::observe};
use editor_widgets::list_view::{ListItem, ListItemContent, ListView};

const ITEM_BG: Color = Color::NONE;
const ITEM_HOVER_BG: Color = Color::srgba(0.2, 0.2, 0.2, 1.0);
const INDEX_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

/// Styled list view container (vertical column with left indent)
pub fn list_view() -> impl Bundle {
    (
        ListView,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::left(px(12.0)),
            ..default()
        },
    )
}

/// Styled list item row: [index] label + content area + hover effects
pub fn list_item(index: usize) -> impl Bundle {
    (
        ListItem { index },
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            padding: UiRect::axes(px(2.0), px(1.0)),
            width: percent(100),
            ..default()
        },
        BackgroundColor(ITEM_BG),
        children![
            // Index label
            (
                Text::new(format!("[{index}]")),
                TextFont {
                    font_size: 11.,
                    ..default()
                },
                TextColor(INDEX_COLOR),
                Node {
                    min_width: px(28.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ),
            // Content placeholder
            (
                ListItemContent,
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            )
        ],
        // Hover effects
        observe(
            |hover: On<Pointer<Over>>,
             mut q: Query<&mut BackgroundColor, With<ListItem>>| {
                if let Ok(mut bg) = q.get_mut(hover.event_target()) {
                    bg.0 = ITEM_HOVER_BG;
                }
            },
        ),
        observe(
            |out: On<Pointer<Out>>,
             mut q: Query<&mut BackgroundColor, With<ListItem>>| {
                if let Ok(mut bg) = q.get_mut(out.event_target()) {
                    bg.0 = ITEM_BG;
                }
            },
        ),
    )
}
