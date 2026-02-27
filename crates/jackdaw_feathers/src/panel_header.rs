use bevy::{feathers::theme::ThemedText, prelude::*};

use crate::tokens;

/// A panel header bar with a title label.
pub fn panel_header(title: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            width: Val::Percent(100.0),
            height: Val::Px(tokens::ROW_HEIGHT),
            padding: UiRect::horizontal(Val::Px(tokens::SPACING_MD)),
            flex_shrink: 0.0,
            ..Default::default()
        },
        BackgroundColor(tokens::PANEL_HEADER_BG),
        children![(
            Text::new(title),
            TextFont {
                font_size: tokens::FONT_MD,
                ..Default::default()
            },
            ThemedText,
        )],
    )
}
