use bevy::prelude::*;
use crate::headless::*;

const INDENT_PX: f32 = 16.0;
const ROW_HEIGHT: f32 = 24.0;
const SELECTED_BG: Color = Color::srgba(0.2, 0.4, 0.7, 0.5);

pub fn tree_row(
    label: &str,
    depth: u32,
    expanded: bool,
    has_children: bool,
    selected: bool,
    source_entity: Option<Entity>,
) -> impl Bundle {
    let bg = if selected {
        BackgroundColor(SELECTED_BG)
    } else {
        BackgroundColor(Color::NONE)
    };

    let arrow = if has_children {
        if expanded { "▼ " } else { "▶ " }
    } else {
        "  "
    };

    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(ROW_HEIGHT),
            padding: UiRect::left(Val::Px(depth as f32 * INDENT_PX)),
            align_items: AlignItems::Center,
            ..default()
        },
        bg,
        Interaction::default(),
        TreeNode {
            expanded,
            depth,
            source_entity,
        },
        children![
            // Expand toggle
            (
                Node {
                    width: Val::Px(16.0),
                    height: Val::Px(ROW_HEIGHT),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                TreeNodeExpandToggle,
                Interaction::default(),
                children![
                    (Text::new(arrow.to_string()), TextFont { font_size: 10.0, ..default() }),
                ],
            ),
            // Label
            (
                Text::new(label.to_string()),
                TextFont { font_size: 13.0, ..default() },
            ),
        ],
    )
}
