use bevy::{feathers::theme::ThemedText, input_focus::InputFocus, prelude::*, ui_widgets::observe};
use editor_widgets::text_input::{TextInput, TextInputDisplay, TextInputPlacholder};

const INPUT_BG: Color = Color::srgba(0.15, 0.15, 0.15, 1.0);
const INPUT_BORDER: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);
// const PLACEHOLDER_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

pub fn text_input(placeholder: impl Into<String>) -> impl Bundle {
    (
        TextInput::default(),
        TextInputPlacholder(placeholder.into()),
        Node {
            width: percent(100.0),
            height: px(28),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            align_items: AlignItems::Center,
            border: px(1).all(),
            ..default()
        },
        BackgroundColor::from(INPUT_BG),
        BorderColor::all(INPUT_BORDER),
        children![(
            TextInputDisplay,
            TextFont {
                font_size: 13.,
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
