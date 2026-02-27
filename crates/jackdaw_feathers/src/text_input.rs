use bevy::{feathers::theme::ThemedText, input_focus::InputFocus, prelude::*, ui_widgets::observe};
use jackdaw_widgets::text_input::{TextInput, TextInputDisplay, TextInputPlacholder};

use crate::tokens;

pub fn text_input(placeholder: impl Into<String>) -> impl Bundle {
    (
        TextInput::default(),
        TextInputPlacholder(placeholder.into()),
        Node {
            width: percent(100.0),
            height: px(tokens::INPUT_HEIGHT),
            padding: UiRect::axes(Val::Px(tokens::SPACING_MD), Val::Px(tokens::SPACING_SM)),
            align_items: AlignItems::Center,
            border: px(1).all(),
            ..default()
        },
        BackgroundColor::from(tokens::INPUT_BG),
        BorderColor::all(tokens::BORDER_SUBTLE),
        children![(
            TextInputDisplay,
            TextFont {
                font_size: tokens::FONT_MD,
                ..Default::default()
            },
            ThemedText
        )],
        observe(
            |click: On<Pointer<Click>>, mut input_focus: ResMut<InputFocus>| {
                input_focus.set(click.entity);
            },
        ),
    )
}
