use std::fmt::Display;

use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

#[derive(Component, Default)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

#[derive(Component, Default)]
pub struct TextInputPlaceholder(pub String);

impl Display for TextInputPlaceholder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TextInputPlaceholder {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Component)]
#[component(on_add)]
pub struct TextInputFocused;
impl TextInputFocused {
    fn on_add(mut world: DeferredWorld, ctx: HookContext) {
        world.commands().queue(move |world: &mut World| {
            let old_entities = world
                .try_query_filtered::<Entity, With<Self>>()
                .unwrap()
                .iter(world)
                .filter(|entity| *entity != ctx.entity)
                // Collect is to ensure that there is no longer an exclusive reference to world
                .collect::<Vec<_>>();

            for entity in old_entities {
                world.entity_mut(entity).remove::<Self>();
            }
        });
    }
}

#[derive(Component)]
pub struct TextInputDisplay;

pub fn text_input_focus(
    click: On<Pointer<Click>>,
    mut commands: Commands,
    text_input: Query<(), With<TextInput>>,
) -> Result<(), BevyError> {
    if !text_input.contains(click.entity) {
        return Ok(());
    }

    commands.entity(click.entity).insert(TextInputFocused);

    Ok(())
}

pub fn text_input_keyboard_system(
    mut keyboard_events: MessageReader<KeyboardInput>,
    selected_input: Single<&mut TextInput, With<TextInputFocused>>,
) {
    let mut input = selected_input.into_inner();

    for event in keyboard_events
        .read()
        .filter(|event| event.state.is_pressed())
    {
        if let Some(text) = &event.text
            && text.chars().all(|c| !c.is_control())
        {
            let cursor = input.cursor;
            input.value.insert_str(cursor, text);
            input.cursor += text.len();
        }

        match &event.logical_key {
            Key::Backspace => {
                let cursor = input.cursor;
                if cursor > 0 {
                    input.value.remove(cursor - 1);
                    input.cursor -= 1;
                }
            }
            Key::Delete => {
                let cursor = input.cursor;
                if cursor < input.value.len() {
                    input.value.remove(cursor);
                }
            }
            Key::ArrowLeft => {
                if input.cursor > 0 {
                    input.cursor -= 1;
                }
            }
            Key::ArrowRight => {
                if input.cursor < input.value.len() {
                    input.cursor += 1;
                }
            }
            Key::Home => {
                input.cursor = 0;
            }
            Key::End => {
                input.cursor = input.value.len();
            }
            _ => {}
        }
    }
}
