use bevy::prelude::*;
pub use lucide_icons::Icon;

/// Resource holding the loaded Lucide icon font handle.
#[derive(Resource)]
pub struct IconFont(pub Handle<Font>);

/// Resource holding the loaded editor body font (InterVariable).
#[derive(Resource)]
pub struct EditorFont(pub Handle<Font>);

pub struct IconFontPlugin;

impl Plugin for IconFontPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_fonts);
    }
}

fn load_fonts(mut commands: Commands, mut fonts: ResMut<Assets<Font>>, asset_server: Res<AssetServer>) {
    // Load icon font from embedded bytes
    let icon_font = Font::try_from_bytes(lucide_icons::LUCIDE_FONT_BYTES.to_vec())
        .expect("Failed to load Lucide icon font");
    let icon_handle = fonts.add(icon_font);
    commands.insert_resource(IconFont(icon_handle));

    // Load InterVariable body font from assets
    let editor_font_handle = asset_server.load("fonts/InterVariable.ttf");
    commands.insert_resource(EditorFont(editor_font_handle));
}

/// Create a text bundle that renders a single Lucide icon glyph.
pub fn icon(icon: Icon, size: f32, font: Handle<Font>) -> impl Bundle {
    (
        Text::new(String::from(icon.unicode())),
        TextFont {
            font,
            font_size: size,
            ..Default::default()
        },
    )
}

/// Create a text bundle for an icon with a specific color.
pub fn icon_colored(icon: Icon, size: f32, font: Handle<Font>, color: Color) -> impl Bundle {
    (
        Text::new(String::from(icon.unicode())),
        TextFont {
            font,
            font_size: size,
            ..Default::default()
        },
        TextColor(color),
    )
}
