use bevy::prelude::*;
use bevy_notify::prelude::*;
use std::fmt::Display;

#[derive(Component, Default)]
#[require(
    MonitorSelf,
    NotifyChanged::<TextInput>::default(),
)]
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
pub struct TextInputDisplay;
