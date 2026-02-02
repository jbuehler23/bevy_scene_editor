use bevy::prelude::*;
use bevy_notify::prelude::*;

#[derive(Component, Default)]
#[require(
    MonitorSelf,
    NotifyChanged::<TextInput>::default(),
)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub default: String,
}
impl TextInput {
    pub fn new(placeholder: impl Into<String>) -> Self {
        Self {
            default: placeholder.into(),
            ..Default::default()
        }
    }
}

#[derive(Component)]
pub struct TextInputDisplay;
