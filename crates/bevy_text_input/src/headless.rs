use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

#[derive(Component)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub placeholder: String,
}

impl Default for TextInput {
    fn default() -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            placeholder: String::new(),
        }
    }
}

#[derive(Component)]
pub struct TextInputFocused;

#[derive(Component)]
pub struct TextInputDisplay;

#[derive(Message)]
pub struct TextInputChanged {
    pub entity: Entity,
    pub value: String,
}

pub fn text_input_focus_system(
    mut commands: Commands,
    query: Query<(Entity, &Interaction), (Changed<Interaction>, With<TextInput>)>,
    focused: Query<Entity, With<TextInputFocused>>,
) {
    for (entity, interaction) in &query {
        if *interaction == Interaction::Pressed {
            for old in &focused {
                commands.entity(old).remove::<TextInputFocused>();
            }
            commands.entity(entity).insert(TextInputFocused);
        }
    }
}

pub fn text_input_keyboard_system(
    mut keyboard_events: MessageReader<KeyboardInput>,
    mut query: Query<(Entity, &mut TextInput), With<TextInputFocused>>,
    mut change_events: MessageWriter<TextInputChanged>,
) {
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        for (entity, mut input) in &mut query {
            let mut changed = false;

            match (&event.logical_key, &event.text) {
                (Key::Backspace, _) => {
                    let cursor = input.cursor;
                    if cursor > 0 {
                        input.value.remove(cursor - 1);
                        input.cursor -= 1;
                        changed = true;
                    }
                }
                (Key::Delete, _) => {
                    let cursor = input.cursor;
                    if cursor < input.value.len() {
                        input.value.remove(cursor);
                        changed = true;
                    }
                }
                (Key::ArrowLeft, _) => {
                    if input.cursor > 0 {
                        input.cursor -= 1;
                    }
                }
                (Key::ArrowRight, _) => {
                    if input.cursor < input.value.len() {
                        input.cursor += 1;
                    }
                }
                (Key::Home, _) => {
                    input.cursor = 0;
                }
                (Key::End, _) => {
                    input.cursor = input.value.len();
                }
                (_, Some(text)) => {
                    if text.chars().all(|c| !c.is_control()) {
                        let cursor = input.cursor;
                        input.value.insert_str(cursor, text);
                        input.cursor += text.len();
                        changed = true;
                    }
                }
                _ => {}
            }

            if changed {
                change_events.write(TextInputChanged {
                    entity,
                    value: input.value.clone(),
                });
            }
        }
    }
}
