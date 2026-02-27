use bevy::{feathers::theme::ThemedText, prelude::*, ui_widgets::observe};
use jackdaw_widgets::numeric_input::{NumericInput, NumericInputDisplay, start_numeric_drag};

use crate::tokens;

pub fn numeric_input(value: f64) -> impl Bundle {
    (
        NumericInput::new(value),
        Node {
            height: Val::Px(tokens::INPUT_HEIGHT),
            padding: UiRect::axes(
                Val::Px(tokens::SPACING_SM),
                Val::Px(tokens::SPACING_XS),
            ),
            align_items: AlignItems::Center,
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            border_radius: BorderRadius::all(Val::Px(tokens::BORDER_RADIUS_SM)),
            ..Default::default()
        },
        BackgroundColor(tokens::INPUT_BG),
        children![(
            NumericInputDisplay,
            Text::new(format!("{value:.3}")),
            TextFont {
                font_size: tokens::FONT_MD,
                ..Default::default()
            },
            ThemedText,
        )],
        observe(start_numeric_drag),
    )
}
