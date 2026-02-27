use std::path::PathBuf;

use bevy::{
    feathers::theme::ThemedText,
    prelude::*,
};
use jackdaw_feathers::{file_browser, icons::IconFont, tokens};
use jackdaw_widgets::file_browser::{FileBrowserItem, FileItemDoubleClicked};

use crate::{
    brush::{BrushEditMode, BrushSelection, EditMode},
    texture_browser::{ApplyTextureToFaces, to_asset_relative_path},
    EditorEntity,
};

pub struct AssetBrowserPlugin;

impl Plugin for AssetBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetBrowserState>()
            .add_systems(Startup, setup_initial_directory)
            .add_systems(Update, refresh_browser_on_change)
            .add_observer(handle_file_double_click);
    }
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BrowserViewMode {
    #[default]
    Grid,
    List,
}

#[derive(Resource)]
pub struct AssetBrowserState {
    pub current_directory: PathBuf,
    pub root_directory: PathBuf,
    pub filter: String,
    pub view_mode: BrowserViewMode,
    pub needs_refresh: bool,
    pub entries: Vec<DirEntry>,
}

impl Default for AssetBrowserState {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            current_directory: cwd.clone(),
            root_directory: cwd,
            filter: String::new(),
            view_mode: BrowserViewMode::Grid,
            needs_refresh: true,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub is_directory: bool,
}

/// Marker for the asset browser panel container.
#[derive(Component)]
pub struct AssetBrowserPanel;

/// Marker for the asset browser content area (where items are displayed).
#[derive(Component)]
pub struct AssetBrowserContent;

/// Marker for the breadcrumb bar.
#[derive(Component)]
pub struct AssetBrowserBreadcrumb;

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn setup_initial_directory(mut state: ResMut<AssetBrowserState>) {
    // Set the assets directory as the root to match Bevy's asset loading paths.
    let assets_dir = state.root_directory.join("assets");
    if assets_dir.is_dir() {
        state.current_directory = assets_dir.clone();
        state.root_directory = assets_dir;
    }
    state.needs_refresh = true;
}

fn refresh_browser_on_change(
    mut state: ResMut<AssetBrowserState>,
    mut commands: Commands,
    icon_font: Res<IconFont>,
    content_query: Query<(Entity, Option<&Children>), With<AssetBrowserContent>>,
    breadcrumb_query: Query<(Entity, Option<&Children>), With<AssetBrowserBreadcrumb>>,
) {
    if !state.needs_refresh {
        return;
    }
    state.needs_refresh = false;

    // Scan directory
    state.entries.clear();
    if let Ok(read_dir) = std::fs::read_dir(&state.current_directory) {
        let mut entries: Vec<DirEntry> = read_dir
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files
                if file_name.starts_with('.') {
                    return None;
                }
                // Apply filter
                if !state.filter.is_empty()
                    && !file_name.to_lowercase().contains(&state.filter.to_lowercase())
                {
                    return None;
                }
                Some(DirEntry {
                    path: entry.path(),
                    file_name,
                    is_directory: entry.file_type().ok()?.is_dir(),
                })
            })
            .collect();

        // Sort: directories first, then alphabetically
        entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()))
        });

        state.entries = entries;
    }

    // Clear content area children
    let Ok((content_entity, content_children)) = content_query.single() else {
        return;
    };
    if let Some(children) = content_children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Spawn items
    for entry in &state.entries {
        let item = FileBrowserItem {
            path: entry.path.to_string_lossy().to_string(),
            is_directory: entry.is_directory,
            file_name: entry.file_name.clone(),
        };

        let path_for_click = entry.path.to_string_lossy().to_string();
        let is_dir = entry.is_directory;

        let item_entity = match state.view_mode {
            BrowserViewMode::Grid => commands
                .spawn((
                    file_browser::file_browser_item(&item, &icon_font),
                    ChildOf(content_entity),
                ))
                .id(),
            BrowserViewMode::List => commands
                .spawn((
                    file_browser::file_browser_list_item(&item, &icon_font),
                    ChildOf(content_entity),
                ))
                .id(),
        };

        // Hover effects
        commands.entity(item_entity).observe(
            |hover: On<Pointer<Over>>, mut bg: Query<&mut BackgroundColor>| {
                if let Ok(mut bg) = bg.get_mut(hover.event_target()) {
                    bg.0 = tokens::HOVER_BG;
                }
            },
        );
        commands.entity(item_entity).observe(
            |out: On<Pointer<Out>>, mut bg: Query<&mut BackgroundColor>| {
                if let Ok(mut bg) = bg.get_mut(out.event_target()) {
                    bg.0 = Color::NONE;
                }
            },
        );
        // Click handler — trigger FileItemDoubleClicked
        commands.entity(item_entity).observe(
            move |_: On<Pointer<Click>>, mut commands: Commands| {
                commands.trigger(FileItemDoubleClicked {
                    path: path_for_click.clone(),
                    is_directory: is_dir,
                });
            },
        );
    }

    // Update breadcrumb
    let Ok((breadcrumb_entity, breadcrumb_children)) = breadcrumb_query.single() else {
        return;
    };
    if let Some(children) = breadcrumb_children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    let relative = state
        .current_directory
        .strip_prefix(&state.root_directory)
        .unwrap_or(&state.current_directory);
    let path_str = relative.to_string_lossy().to_string();

    commands.spawn((
        Text::new(if path_str.is_empty() {
            "/".to_string()
        } else {
            format!("/ {}", path_str.replace(std::path::MAIN_SEPARATOR, " / "))
        }),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        ThemedText,
        ChildOf(breadcrumb_entity),
    ));
}

fn handle_file_double_click(
    event: On<FileItemDoubleClicked>,
    mut state: ResMut<AssetBrowserState>,
    edit_mode: Res<EditMode>,
    brush_selection: Res<BrushSelection>,
    mut commands: Commands,
) {
    if event.is_directory {
        state.current_directory = PathBuf::from(&event.path);
        state.needs_refresh = true;
        return;
    }

    // If in face edit mode with faces selected and double-clicking an image, apply it
    if *edit_mode == EditMode::BrushEdit(BrushEditMode::Face)
        && !brush_selection.faces.is_empty()
        && brush_selection.entity.is_some()
    {
        if is_image_file(&event.path) {
            if let Some(relative) = to_asset_relative_path(&event.path) {
                commands.trigger(ApplyTextureToFaces { path: relative });
            }
        }
    }
}

fn is_image_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    path_lower.ends_with(".png")
        || path_lower.ends_with(".jpg")
        || path_lower.ends_with(".jpeg")
        || path_lower.ends_with(".bmp")
        || path_lower.ends_with(".tga")
        || path_lower.ends_with(".webp")
}

// ---------------------------------------------------------------------------
// Layout helper — creates the asset browser panel bundle
// ---------------------------------------------------------------------------

pub fn asset_browser_panel() -> impl Bundle {
    (
        AssetBrowserPanel,
        EditorEntity,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(tokens::SPACING_SM)),
            ..Default::default()
        },
        BackgroundColor(tokens::PANEL_BG),
        children![
            // Breadcrumb bar
            (
                AssetBrowserBreadcrumb,
                EditorEntity,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(tokens::SPACING_MD), Val::Px(tokens::SPACING_SM)),
                    width: Val::Percent(100.0),
                    height: Val::Px(tokens::HEADER_HEIGHT),
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                BackgroundColor(tokens::TOOLBAR_BG),
            ),
            // Content area
            (
                AssetBrowserContent,
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
                    row_gap: Val::Px(tokens::SPACING_SM),
                    column_gap: Val::Px(tokens::SPACING_SM),
                    ..Default::default()
                },
            )
        ],
    )
}
