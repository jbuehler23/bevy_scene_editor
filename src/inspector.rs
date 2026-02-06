use crate::EditorEntity;
use bevy::{
    ecs::{
        archetype::Archetype,
        component::{ComponentId, Components},
        lifecycle::HookContext,
        world::DeferredWorld,
    },
    feathers::{
        constants::size,
        controls::{ButtonProps, button},
        theme::ThemedText,
    },
    prelude::*,
    ui_widgets::observe,
};
use editor_feathers::text_input;
use editor_widgets::text_input::{EnteredText, TextInput};

#[reflect_trait]
pub trait Displayable {
    fn display(&self, entity: &mut EntityCommands, source: Entity);
}

impl Displayable for Name {
    fn display(&self, entity: &mut EntityCommands, source: Entity) {
        entity
            .insert(text_input::text_input("Name..."))
            .insert(TextInput::new(self.to_string()))
            .observe(
                move |text: On<EnteredText>,
                      mut names: Query<&mut Name>|
                      -> Result<(), BevyError> {
                    let mut name = names.get_mut(source)?;

                    *name = Name::new(text.value.clone());

                    Ok(())
                },
            );
    }
}

#[derive(Component)]
#[component(on_add)]
pub struct SelectedEntity;
impl SelectedEntity {
    pub fn on_add(mut world: DeferredWorld, ctx: HookContext) {
        let previous = world
            .try_query_filtered::<Entity, With<Self>>()
            .unwrap()
            .iter(&world)
            .filter(|entity| *entity != ctx.entity)
            .collect::<Vec<_>>();

        world.commands().queue(|world: &mut World| {
            for entity in previous {
                world.entity_mut(entity).remove::<Self>();
            }
        });
    }
}

#[derive(Component)]
#[require(EditorEntity)]
pub struct Inspector;

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(remove_component_displays)
            .add_observer(add_component_displays);
    }
}

fn add_component_displays(
    _: On<Add, SelectedEntity>,
    mut commands: Commands,
    components: &Components,
    selected_entity: Single<(&Archetype, EntityRef), (With<SelectedEntity>, Without<EditorEntity>)>,
    inspector: Single<Entity, With<Inspector>>,
) {
    let (archetype, entity_ref) = selected_entity.into_inner();

    let mut components = archetype
        .iter_components()
        .filter_map(|component| {
            components
                .get_name(component)
                .filter(|name| !name.starts_with("bevy_scene_editor"))
                .map(|name| name.shortname().to_string())
                .zip(Some(component))
        })
        .collect::<Vec<_>>();

    components.sort_by_key(|(name, _)| name.to_lowercase());

    for (name, component_id) in components {
        commands.spawn((
            component_display(name, entity_ref.entity(), component_id),
            ChildOf(*inspector),
        ));
    }

    commands.spawn((
        button(ButtonProps::default(), (), Spawn(Text::new("+"))),
        ChildOf(*inspector),
    ));
}

fn remove_component_displays(
    _: On<Remove, SelectedEntity>,
    mut commands: Commands,
    inspector: Single<(Entity, &Children), With<Inspector>>,
    component_displays: Query<Entity, With<ComponentDisplay>>,
) {
    let (entity, children) = inspector.into_inner();

    let component_displays = component_displays
        .iter_many(children.collection())
        .collect::<Vec<_>>();

    commands.entity(entity).detach_children(&component_displays);
}

const INPUT_BORDER: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);

#[derive(Component)]
pub struct ComponentDisplay;

fn component_display(
    name: impl Into<String>,
    entity: Entity,
    component: ComponentId,
) -> impl Bundle {
    (
        ComponentDisplay,
        Node {
            flex_direction: FlexDirection::Column,
            width: percent(100),
            border: px(2).all(),
            border_radius: BorderRadius::all(px(5)),
            ..Default::default()
        },
        BorderColor::all(INPUT_BORDER),
        children![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    ..Default::default()
                },
                BackgroundColor(INPUT_BORDER),
                children![
                    (
                        Text::new(">"),
                        TextFont {
                            font_size: 13.,
                            ..Default::default()
                        },
                        ThemedText
                    ),
                    (
                        Text::new(name),
                        TextFont {
                            font_size: 13.,
                            ..Default::default()
                        },
                        ThemedText
                    ),
                    (
                        Text::new("-"),
                        TextFont {
                            font_size: 13.,
                            ..Default::default()
                        },
                        ThemedText,
                        observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
                            commands.entity(entity).remove_by_id(component);
                        })
                    )
                ]
            ),
            (
                Node {
                    padding: percent(2).all(),
                    ..Default::default()
                },
                children![Text::new("Component Info here")]
            )
        ],
    )
}
