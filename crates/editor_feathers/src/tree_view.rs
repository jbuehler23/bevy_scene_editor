use bevy::{feathers::theme::ThemedText, prelude::*, ui_widgets::observe};
use editor_widgets::tree_view::{
    TreeNode, TreeNodeExpandToggle, TreeNodeExpanded, TreeRowChildren, TreeRowClicked,
    TreeRowContent, TreeRowDropped, TreeRowDroppedOnRoot, TreeRowLabel, TreeRowSelected,
};

pub const ROW_BG: Color = Color::NONE;
pub const ROW_HOVER_BG: Color = Color::srgba(0.25, 0.25, 0.25, 1.0);
pub const ROW_SELECTED_BG: Color = Color::srgba(0.2, 0.4, 0.6, 1.0);
pub const ROW_DROP_TARGET_BG: Color = Color::srgba(0.3, 0.5, 0.2, 1.0);
pub const DROP_TARGET_BORDER: Color = Color::srgba(0.3, 0.7, 0.4, 1.0);
pub const CONTAINER_DROP_TARGET_BG: Color = Color::srgba(0.2, 0.3, 0.2, 0.3);
const INDENT_WIDTH: f32 = 16.0;
const TOGGLE_WIDTH: f32 = 16.0;

/// Walk up the ChildOf chain from any UI entity until we find a TreeNode,
/// then return its source entity. Handles drags starting on label text,
/// toggle, or any nested child of the tree row.
fn find_source_entity(
    entity: Entity,
    parents: &Query<&ChildOf>,
    tree_nodes: &Query<&TreeNode>,
) -> Option<Entity> {
    let mut current = entity;
    for _ in 0..8 {
        if let Ok(node) = tree_nodes.get(current) {
            return Some(node.0);
        }
        let Ok(&ChildOf(parent)) = parents.get(current) else {
            break;
        };
        current = parent;
    }
    None
}

/// Creates a tree row bundle for displaying an entity in the hierarchy
pub fn tree_row(label: &str, has_children: bool, selected: bool, source: Entity) -> impl Bundle {
    (
        TreeNode(source),
        TreeNodeExpanded(false),
        Node {
            flex_direction: FlexDirection::Column,
            width: percent(100),
            ..default()
        },
        children![
            // The clickable row content
            tree_row_content(label, has_children, selected, source),
            // Container for child rows (initially empty, populated reactively)
            (
                TreeRowChildren,
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::left(px(INDENT_WIDTH)),
                    width: percent(100),
                    ..default()
                }
            )
        ],
    )
}

fn tree_row_content(
    label: &str,
    has_children: bool,
    selected: bool,
    source: Entity,
) -> impl Bundle {
    let bg = if selected { ROW_SELECTED_BG } else { ROW_BG };

    (
        TreeRowContent,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(4.0), px(2.0)),
            width: percent(100),
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(DROP_TARGET_BORDER),
        children![
            // Expand toggle
            expand_toggle(has_children),
            // Label
            (
                TreeRowLabel,
                Text::new(label),
                TextFont {
                    font_size: 13.,
                    ..default()
                },
                ThemedText,
            )
        ],
        // Click handler for selection
        observe(move |click: On<Pointer<Click>>, mut commands: Commands| {
            commands.trigger(TreeRowClicked {
                entity: click.event_target(),
                source_entity: source,
            });
        }),
        // Hover effects (skip selected rows)
        observe(
            |hover: On<Pointer<Over>>,
             mut bg_query: Query<&mut BackgroundColor, (With<TreeRowContent>, Without<TreeRowSelected>)>| {
                if let Ok(mut bg) = bg_query.get_mut(hover.event_target()) {
                    bg.0 = ROW_HOVER_BG;
                }
            },
        ),
        observe(
            |out: On<Pointer<Out>>,
             mut bg_query: Query<&mut BackgroundColor, (With<TreeRowContent>, Without<TreeRowSelected>)>| {
                if let Ok(mut bg) = bg_query.get_mut(out.event_target()) {
                    bg.0 = ROW_BG;
                }
            },
        ),
        // Drag-and-drop: highlight drop target with border accent
        observe(
            |mut drag_enter: On<Pointer<DragEnter>>,
             mut query: Query<(&mut BackgroundColor, &mut Node), With<TreeRowContent>>| {
                drag_enter.propagate(false);
                if let Ok((mut bg, mut node)) = query.get_mut(drag_enter.event_target()) {
                    bg.0 = ROW_DROP_TARGET_BG;
                    node.border = UiRect::left(px(3.0));
                }
            },
        ),
        observe(
            |mut drag_leave: On<Pointer<DragLeave>>,
             mut query: Query<(&mut BackgroundColor, &mut Node), With<TreeRowContent>>,
             selected: Query<(), With<TreeRowSelected>>| {
                drag_leave.propagate(false);
                if let Ok((mut bg, mut node)) = query.get_mut(drag_leave.event_target()) {
                    bg.0 = if selected.contains(drag_leave.event_target()) {
                        ROW_SELECTED_BG
                    } else {
                        ROW_BG
                    };
                    node.border = UiRect::ZERO;
                }
            },
        ),
        // Drag-and-drop: resolve source entities and fire TreeRowDropped
        observe(
            |mut drag_drop: On<Pointer<DragDrop>>,
             mut commands: Commands,
             parent_query: Query<&ChildOf>,
             tree_nodes: Query<&TreeNode>,
             mut query: Query<(&mut BackgroundColor, &mut Node), With<TreeRowContent>>,
             selected_query: Query<(), With<TreeRowSelected>>| {
                drag_drop.propagate(false);
                let target_content = drag_drop.event_target();

                // Revert drop target styling
                if let Ok((mut bg, mut node)) = query.get_mut(target_content) {
                    bg.0 = if selected_query.contains(target_content) {
                        ROW_SELECTED_BG
                    } else {
                        ROW_BG
                    };
                    node.border = UiRect::ZERO;
                }

                // Resolve both target and dragged to their scene source entities
                let Ok(&ChildOf(target_tree_row)) = parent_query.get(target_content) else {
                    return;
                };
                let Ok(target_node) = tree_nodes.get(target_tree_row) else {
                    return;
                };
                let Some(dragged_source) =
                    find_source_entity(drag_drop.dropped, &parent_query, &tree_nodes)
                else {
                    return;
                };

                commands.trigger(TreeRowDropped {
                    entity: target_content,
                    dragged_source,
                    target_source: target_node.0,
                });
            },
        ),
    )
}

fn expand_toggle(has_children: bool) -> impl Bundle {
    let text = if has_children { ">" } else { " " };

    (
        TreeNodeExpandToggle,
        Node {
            width: px(TOGGLE_WIDTH),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Text::new(text),
            TextFont {
                font_size: 11.,
                ..default()
            },
            ThemedText,
        )],
    )
}

/// Returns observers for the root tree container to handle deparenting (drop-to-root).
pub fn tree_container_drop_observers() -> impl Bundle {
    (
        observe(
            |mut drag_enter: On<Pointer<DragEnter>>,
             mut bg_query: Query<&mut BackgroundColor>| {
                drag_enter.propagate(false);
                if let Ok(mut bg) = bg_query.get_mut(drag_enter.event_target()) {
                    bg.0 = CONTAINER_DROP_TARGET_BG;
                }
            },
        ),
        observe(
            |mut drag_leave: On<Pointer<DragLeave>>,
             mut bg_query: Query<&mut BackgroundColor>| {
                drag_leave.propagate(false);
                if let Ok(mut bg) = bg_query.get_mut(drag_leave.event_target()) {
                    bg.0 = Color::NONE;
                }
            },
        ),
        observe(
            |mut drag_drop: On<Pointer<DragDrop>>,
             mut commands: Commands,
             parent_query: Query<&ChildOf>,
             tree_nodes: Query<&TreeNode>,
             mut bg_query: Query<&mut BackgroundColor>| {
                drag_drop.propagate(false);
                let container = drag_drop.event_target();

                // Revert background
                if let Ok(mut bg) = bg_query.get_mut(container) {
                    bg.0 = Color::NONE;
                }

                // Resolve the dragged entity to its scene source
                let Some(dragged_source) =
                    find_source_entity(drag_drop.dropped, &parent_query, &tree_nodes)
                else {
                    return;
                };

                commands.trigger(TreeRowDroppedOnRoot {
                    entity: container,
                    dragged_source,
                });
            },
        ),
    )
}
