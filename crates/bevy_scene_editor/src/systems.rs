use bevy::prelude::*;
use bevy_split_panel::*;
use crate::state::EditorState;

#[derive(Component)]
pub struct SelectionHighlight;

pub fn selection_highlight_system(
    mut commands: Commands,
    editor_state: Res<EditorState>,
    highlighted: Query<Entity, With<SelectionHighlight>>,
) {
    if !editor_state.is_changed() {
        return;
    }

    for entity in &highlighted {
        commands.entity(entity).remove::<SelectionHighlight>();
    }

    if let Some(selected) = editor_state.selected_entity {
        commands.entity(selected).insert(SelectionHighlight);
    }
}

pub fn sync_split_panel_sizes(
    panels: Query<(&SplitPanel, &Children), Changed<SplitPanel>>,
    mut nodes: Query<&mut Node>,
    first_query: Query<(), With<SplitPanelFirst>>,
    second_query: Query<(), With<SplitPanelSecond>>,
) {
    for (panel, children) in &panels {
        let first_pct = panel.ratio * 100.0;
        let second_pct = (1.0 - panel.ratio) * 100.0;

        for child in children.iter() {
            if first_query.contains(child) {
                if let Ok(mut node) = nodes.get_mut(child) {
                    match panel.direction {
                        SplitDirection::Horizontal => node.width = Val::Percent(first_pct),
                        SplitDirection::Vertical => node.height = Val::Percent(first_pct),
                    }
                }
            } else if second_query.contains(child) {
                if let Ok(mut node) = nodes.get_mut(child) {
                    match panel.direction {
                        SplitDirection::Horizontal => node.width = Val::Percent(second_pct),
                        SplitDirection::Vertical => node.height = Val::Percent(second_pct),
                    }
                }
            }
        }
    }
}
