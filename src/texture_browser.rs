use std::path::Path;

use bevy::prelude::*;
use jackdaw_feathers::{panel_header, text_input, tokens};
use jackdaw_widgets::text_input::TextInput;

use crate::{
    brush::{Brush, BrushEditMode, BrushSelection, EditMode, LastUsedTexture, SetBrush},
    commands::CommandHistory,
    EditorEntity,
};

pub struct TextureBrowserPlugin;

impl Plugin for TextureBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvailableTextures>()
            .add_systems(Startup, scan_textures)
            .add_systems(
                Update,
                (rescan_textures, apply_texture_filter, update_texture_browser_ui),
            )
            .add_observer(handle_apply_texture);
    }
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct AvailableTextures {
    pub textures: Vec<TextureEntry>,
    pub needs_rescan: bool,
    pub filter: String,
}

pub struct TextureEntry {
    pub path: String,
    pub file_name: String,
    pub image: Handle<Image>,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Apply a texture to currently selected brush faces.
#[derive(Event, Debug, Clone)]
pub struct ApplyTextureToFaces {
    pub path: String,
}

/// Clear texture from currently selected brush faces.
#[derive(Event, Debug, Clone)]
pub struct ClearTextureFromFaces;

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn scan_textures(
    mut available: ResMut<AvailableTextures>,
    asset_server: Res<AssetServer>,
) {
    do_scan_textures(&mut available, &asset_server);
}

fn do_scan_textures(available: &mut AvailableTextures, asset_server: &AssetServer) {
    available.textures.clear();

    let assets_dir = std::env::current_dir()
        .unwrap_or_default()
        .join("assets");

    if !assets_dir.is_dir() {
        return;
    }

    scan_directory(&assets_dir, &assets_dir, asset_server, &mut available.textures);

    // Sort alphabetically
    available.textures.sort_by(|a, b| a.file_name.cmp(&b.file_name));
}

fn scan_directory(
    dir: &Path,
    root: &Path,
    asset_server: &AssetServer,
    entries: &mut Vec<TextureEntry>,
) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_directory(&path, root, asset_server, entries);
        } else if is_image_file(&path) {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let image: Handle<Image> = asset_server.load(relative.clone());
            entries.push(TextureEntry {
                path: relative,
                file_name,
                image,
            });
        }
    }
}

fn is_image_file(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    let ext = ext.to_string_lossy().to_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "tga" | "webp")
}

fn rescan_textures(
    mut available: ResMut<AvailableTextures>,
    asset_server: Res<AssetServer>,
) {
    if !available.needs_rescan {
        return;
    }
    available.needs_rescan = false;
    do_scan_textures(&mut available, &asset_server);
}

fn handle_apply_texture(
    event: On<ApplyTextureToFaces>,
    brush_selection: Res<BrushSelection>,
    edit_mode: Res<EditMode>,
    mut brushes: Query<&mut Brush>,
    mut history: ResMut<CommandHistory>,
    mut last_texture: ResMut<LastUsedTexture>,
) {
    if *edit_mode != EditMode::BrushEdit(BrushEditMode::Face) {
        return;
    }
    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    if brush_selection.faces.is_empty() {
        return;
    }
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    let old = brush.clone();
    for &face_idx in &brush_selection.faces {
        if face_idx < brush.faces.len() {
            brush.faces[face_idx].texture_path = Some(event.path.clone());
        }
    }

    // Remember the last-used texture for new brushes
    last_texture.texture_path = Some(event.path.clone());

    let cmd = SetBrush {
        entity: brush_entity,
        old,
        new: brush.clone(),
        label: "Apply texture".to_string(),
    };
    history.undo_stack.push(Box::new(cmd));
    history.redo_stack.clear();
}

// ---------------------------------------------------------------------------
// Texture browser UI
// ---------------------------------------------------------------------------

/// Marker for the texture browser panel.
#[derive(Component)]
pub struct TextureBrowserPanel;

/// Marker for the texture browser grid content area.
#[derive(Component)]
pub struct TextureBrowserGrid;

/// Marker for the texture browser filter input.
#[derive(Component)]
pub struct TextureBrowserFilter;

/// Marker for each texture thumbnail.
#[derive(Component)]
pub struct TextureThumbnail {
    pub path: String,
}

/// Update the filter string from the text input.
fn apply_texture_filter(
    filter_input: Query<&TextInput, (With<TextureBrowserFilter>, Changed<TextInput>)>,
    mut available: ResMut<AvailableTextures>,
) {
    for input in &filter_input {
        if available.filter != input.value {
            available.filter = input.value.clone();
        }
    }
}

fn update_texture_browser_ui(
    mut commands: Commands,
    available: Res<AvailableTextures>,
    grid_query: Query<(Entity, Option<&Children>), With<TextureBrowserGrid>>,
) {
    if !available.is_changed() {
        return;
    }

    let Ok((grid_entity, grid_children)) = grid_query.single() else {
        return;
    };

    // Clear existing thumbnails
    if let Some(children) = grid_children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    let filter_lower = available.filter.to_lowercase();

    for entry in &available.textures {
        // Apply filter
        if !filter_lower.is_empty()
            && !entry.file_name.to_lowercase().contains(&filter_lower)
            && !entry.path.to_lowercase().contains(&filter_lower)
        {
            continue;
        }

        let path = entry.path.clone();
        let image = entry.image.clone();

        // Thumbnail container
        let thumb_entity = commands
            .spawn((
                TextureThumbnail {
                    path: path.clone(),
                },
                Node {
                    width: Val::Px(64.0),
                    height: Val::Px(80.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(2.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..Default::default()
                },
                BorderColor::all(Color::NONE),
                BackgroundColor(Color::NONE),
                ChildOf(grid_entity),
            ))
            .id();

        // Image thumbnail
        commands.spawn((
            ImageNode::new(image),
            Node {
                width: Val::Px(56.0),
                height: Val::Px(56.0),
                ..Default::default()
            },
            ChildOf(thumb_entity),
        ));

        // File name label
        let display_name = if entry.file_name.len() > 10 {
            format!("{}...", &entry.file_name[..8])
        } else {
            entry.file_name.clone()
        };
        commands.spawn((
            Text::new(display_name),
            TextFont {
                font_size: 9.0,
                ..Default::default()
            },
            TextColor(tokens::TEXT_SECONDARY),
            Node {
                max_width: Val::Px(60.0),
                overflow: Overflow::clip(),
                ..Default::default()
            },
            ChildOf(thumb_entity),
        ));

        // Hover + click
        let path_for_click = path.clone();
        commands.entity(thumb_entity).observe(
            |hover: On<Pointer<Over>>, mut borders: Query<&mut BorderColor>| {
                if let Ok(mut border) = borders.get_mut(hover.event_target()) {
                    *border = BorderColor::all(tokens::SELECTED_BORDER);
                }
            },
        );
        commands.entity(thumb_entity).observe(
            |out: On<Pointer<Out>>, mut borders: Query<&mut BorderColor>| {
                if let Ok(mut border) = borders.get_mut(out.event_target()) {
                    *border = BorderColor::all(Color::NONE);
                }
            },
        );
        commands.entity(thumb_entity).observe(
            move |_: On<Pointer<Click>>, mut commands: Commands| {
                commands.trigger(ApplyTextureToFaces {
                    path: path_for_click.clone(),
                });
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Layout helper
// ---------------------------------------------------------------------------

pub fn texture_browser_panel() -> impl Bundle {
    (
        TextureBrowserPanel,
        EditorEntity,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        },
        BackgroundColor(tokens::PANEL_BG),
        children![
            // Header
            panel_header::panel_header("Textures"),
            // Filter input
            (
                Node {
                    padding: UiRect::axes(Val::Px(tokens::SPACING_SM), Val::Px(tokens::SPACING_XS)),
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                children![
                    (TextureBrowserFilter, text_input::text_input("Filter textures")),
                ],
            ),
            // Grid
            (
                TextureBrowserGrid,
                EditorEntity,
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_content: AlignContent::FlexStart,
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::all(Val::Px(tokens::SPACING_SM)),
                    row_gap: Val::Px(tokens::SPACING_XS),
                    column_gap: Val::Px(tokens::SPACING_XS),
                    ..Default::default()
                },
            ),
        ],
    )
}

/// Convert an absolute filesystem path to an asset-relative path.
pub fn to_asset_relative_path(absolute: &str) -> Option<String> {
    let assets_dir = std::env::current_dir()
        .ok()?
        .join("assets");
    let abs_path = Path::new(absolute);
    let relative = abs_path
        .strip_prefix(&assets_dir)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some(relative)
}
