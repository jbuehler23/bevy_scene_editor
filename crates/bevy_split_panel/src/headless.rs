use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitDirection {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Component, Debug)]
pub struct SplitPanel {
    pub direction: SplitDirection,
    pub ratio: f32,
    pub min_size: f32,
}

impl Default for SplitPanel {
    fn default() -> Self {
        Self {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            min_size: 50.0,
        }
    }
}

#[derive(Component)]
pub struct SplitPanelFirst;

#[derive(Component)]
pub struct SplitPanelSecond;

#[derive(Component)]
pub struct SplitHandle;

#[derive(Component)]
pub struct SplitHandleDragging;

pub fn split_panel_start_drag(
    query: Query<(Entity, &Interaction), (Changed<Interaction>, With<SplitHandle>)>,
    mut commands: Commands,
) {
    for (entity, interaction) in &query {
        if *interaction == Interaction::Pressed {
            commands.entity(entity).insert(SplitHandleDragging);
        }
    }
}

pub fn split_panel_stop_drag(
    mouse_button: Res<ButtonInput<MouseButton>>,
    dragging: Query<Entity, With<SplitHandleDragging>>,
    mut commands: Commands,
) {
    if mouse_button.just_released(MouseButton::Left) {
        for entity in &dragging {
            commands.entity(entity).remove::<SplitHandleDragging>();
        }
    }
}

pub fn split_panel_drag_system(
    mut cursor_moved: MessageReader<CursorMoved>,
    dragging: Query<Entity, With<SplitHandleDragging>>,
    children_query: Query<&ChildOf>,
    mut panels: Query<(&mut SplitPanel, &ComputedNode, &GlobalTransform)>,
) {
    for event in cursor_moved.read() {
        for handle_entity in &dragging {
            if let Ok(child_of) = children_query.get(handle_entity) {
                let parent = child_of.parent();
                if let Ok((mut panel, computed, transform)) = panels.get_mut(parent) {
                    let size = computed.size();
                    let pos = transform.translation().truncate();
                    let cursor = event.position;

                    let new_ratio = match panel.direction {
                        SplitDirection::Horizontal => {
                            let local_x = cursor.x - (pos.x - size.x / 2.0);
                            (local_x / size.x).clamp(0.0, 1.0)
                        }
                        SplitDirection::Vertical => {
                            let local_y = cursor.y - (pos.y - size.y / 2.0);
                            (local_y / size.y).clamp(0.0, 1.0)
                        }
                    };

                    let total = match panel.direction {
                        SplitDirection::Horizontal => size.x,
                        SplitDirection::Vertical => size.y,
                    };

                    if total > 0.0 {
                        let min_ratio = panel.min_size / total;
                        let max_ratio = 1.0 - min_ratio;
                        panel.ratio = new_ratio.clamp(min_ratio, max_ratio);
                    }
                }
            }
        }
    }
}
