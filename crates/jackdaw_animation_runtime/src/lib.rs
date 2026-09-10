//! Playing an authored set of animation states on a skeleton loaded from glTF.
//!
//! A rig is rarely one file. The clips are exported once against a reference
//! armature, and the bodies that play them are exported separately, so the
//! animation player cannot be built by whatever loaded either half; it has to
//! be built once both have spawned. This crate holds that binding step, the
//! small state machine an author writes beside it, and the joint remapping
//! that lets several parts wear one skeleton.
//!
//! Clips address bones by a hash of their name path rather than by entity, so
//! a clip exported against one armature drives any armature whose bones carry
//! the same names. [`AnimationRuntimePlugin`] writes those ids itself because
//! Bevy's glTF loader only writes them for a file that carries animations of
//! its own, which a body exported without clips does not.

#![deny(missing_docs)]

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use bevy::{
    animation::{
        ActiveAnimation, AnimatedBy, AnimationTargetId, RepeatAnimation,
        graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex},
        transition::AnimationTransitions,
    },
    asset::AssetPlugin,
    gltf::Gltf,
    mesh::skinning::SkinnedMesh,
    prelude::*,
};
use serde::{Deserialize, Serialize};

pub mod events;
pub mod graph;

pub use events::{AnimationEvent, ClipEvent, ClipPass, ClipPlayhead, fire_clip_events};
pub use graph::{
    AnimationBlendPoint, AnimationClipRef, AnimationCondition, AnimationConditionOp,
    AnimationGraphAsset, AnimationGraphBound, AnimationGraphDef, AnimationGraphLoadError,
    AnimationGraphLoader, AnimationGraphPlayback, AnimationGraphRef, AnimationGraphSource,
    AnimationGraphState, AnimationGraphTransition, AnimationMotion, AnimationParameterDef,
    AnimationParameterKind, AnimationParams, AnimationTransitionDef, parse_animation_graph,
    register_animation_graph_types,
};

/// The clips an entity can play and the states that choose between them.
///
/// A component equal to its `Default` emits as a bare type path, so changing
/// this `Default` silently reinterprets every scene already saved that way.
#[derive(Component, Reflect, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[reflect(Component, Default)]
pub struct AnimationSet {
    /// Assets-relative paths of the glTF files holding the clips. A state
    /// names one of them by its position in this list.
    pub sources: Vec<String>,
    /// Every state this set can be asked for.
    pub states: Vec<AnimationStateDef>,
    /// The state played as soon as the set binds. Empty asks for nothing.
    pub default_state: String,
    /// Name of the descendant carrying the skeleton the clips drive.
    pub skeleton_root: String,
}

impl Default for AnimationSet {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            states: Vec::new(),
            default_state: String::new(),
            skeleton_root: "Armature".to_string(),
        }
    }
}

/// One state of an [`AnimationSet`]: which clip it plays, and how.
#[derive(Reflect, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[reflect(Default)]
pub struct AnimationStateDef {
    /// What an [`AnimationState`] names to ask for this state.
    pub name: String,
    /// Index into [`AnimationSet::sources`] of the file holding the clip.
    pub source: usize,
    /// Name the clip carries in that file.
    pub clip: String,
    /// Whether the clip runs forever or once.
    pub looped: bool,
    /// Seconds over which the state it replaces fades out.
    pub transition_secs: f32,
    /// Playback rate, as a multiple of the clip's authored speed.
    pub speed: f32,
    /// State to fall back to once a non-looping clip has run out.
    pub then: Option<String>,
}

impl Default for AnimationStateDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            source: 0,
            clip: String::new(),
            looped: true,
            transition_secs: 0.15,
            speed: 1.0,
            then: None,
        }
    }
}

/// The state an [`AnimationSet`] is being asked to play.
///
/// Whatever drives the entity writes this: game logic in a running world, the
/// operator that previews a state in the editor. Empty asks for nothing, so a
/// set with no default holds whatever pose its skeleton spawned in.
#[derive(Component, Reflect, Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct AnimationState(
    /// Name of the wanted [`AnimationStateDef`].
    pub String,
);

/// Handles to the glTF files an [`AnimationSet`] draws its clips from.
///
/// Held on the entity so the files stay loaded for as long as the graph built
/// out of them is playing. Inserting it before the set binds skips the load,
/// which is what an app that already holds the files should do.
#[derive(Component, Debug, Default)]
pub struct AnimationSources(
    /// One handle per entry of [`AnimationSet::sources`], in the same order.
    pub Vec<Handle<Gltf>>,
);

/// What an [`AnimationSet`] resolved to once its skeleton and its clips were
/// both in the world.
///
/// Runtime only: it names entities and holds asset handles, so it is neither
/// reflected nor written to a document.
#[derive(Component, Debug)]
pub struct AnimationSetBound {
    /// The skeleton root, which carries the [`AnimationPlayer`].
    pub player: Entity,
    /// The graph node each playable state was added as.
    pub nodes: HashMap<String, AnimationNodeIndex>,
    /// The graph every animated root under this set shares.
    pub graph: Handle<AnimationGraph>,
    /// Further animated roots: parts whose skeleton could not be folded into
    /// the primary one and so kept their own.
    pub parts: Vec<Entity>,
    /// State names already reported as unknown, so asking again stays quiet.
    pub warned_states: HashSet<String>,
}

/// Sent when a non-looping state reaches the end of its clip.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct AnimationStateFinished {
    /// The entity carrying the [`AnimationSet`] or the [`AnimationGraphRef`].
    pub entity: Entity,
    /// The state that ran out.
    pub state: String,
}

/// The systems that bind animation sets and graphs and play the state they
/// are asked for.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnimationSetSystems;

/// Binds authored animation sets and graphs to their skeletons and plays the
/// state each asks for.
///
/// Add it wherever scenes carrying an [`AnimationSet`] or an
/// [`AnimationGraphRef`] are spawned. Nothing here touches an entity carrying
/// neither.
pub struct AnimationRuntimePlugin;

impl Plugin for AnimationRuntimePlugin {
    fn build(&self, app: &mut App) {
        register_animation_set_types(app);
        graph::register_animation_graph_types(app);
        let graph_assets_can_be_loaded = app.is_plugin_added::<AssetPlugin>();
        if graph_assets_can_be_loaded {
            app.init_asset::<AnimationGraphAsset>()
                .init_asset_loader::<AnimationGraphLoader>();
        }
        app.add_message::<AnimationStateFinished>()
            .add_message::<AnimationEvent>()
            .add_systems(
                Update,
                (
                    bind_animation_sets,
                    merge_part_skeletons,
                    apply_animation_state,
                    report_finished_states,
                )
                    .chain()
                    .in_set(AnimationSetSystems)
                    .run_if(
                        resource_exists::<AssetServer>
                            .and_then(resource_exists::<Assets<Gltf>>)
                            .and_then(resource_exists::<Assets<AnimationGraph>>),
                    ),
            )
            .add_systems(
                Update,
                (
                    graph::rebind_edited_graphs,
                    graph::bind_animation_graphs,
                    graph::advance_animation_graphs,
                )
                    .chain()
                    .in_set(AnimationSetSystems)
                    .run_if(
                        resource_exists::<AssetServer>
                            .and_then(resource_exists::<Assets<Gltf>>)
                            .and_then(resource_exists::<Assets<AnimationGraph>>)
                            .and_then(resource_exists::<Assets<AnimationGraphAsset>>),
                    ),
            )
            .add_systems(
                Update,
                (write_clip_playheads, fire_clip_events)
                    .chain()
                    .in_set(AnimationSetSystems)
                    .after(apply_animation_state)
                    .after(graph::advance_animation_graphs)
                    .before(report_finished_states),
            );
    }
}

/// Registers the authored animation types for reflection.
///
/// [`AnimationRuntimePlugin`] calls this; call it directly only to author and
/// load sets in an app that never plays them, such as one that only writes
/// documents.
pub fn register_animation_set_types(app: &mut App) {
    app.register_type::<AnimationSet>()
        .register_type::<AnimationStateDef>()
        .register_type::<AnimationState>()
        .register_type::<ClipEvent>();
}

/// Builds the player, the graph and the bone target ids of every set whose
/// skeleton and source files have arrived.
///
/// An entity naming a graph file is left to the graph evaluator: the two would
/// otherwise each claim the same skeleton's player. A graph reference naming
/// no file yet is not one.
///
/// Retried each frame rather than run on insertion, because a glTF scene
/// spawns asynchronously and its skeleton can be several frames behind the
/// component naming it.
fn bind_animation_sets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    unbound: Query<
        (
            Entity,
            &AnimationSet,
            Option<&AnimationGraphRef>,
            Option<&AnimationSources>,
            Option<&AnimationState>,
        ),
        Without<AnimationSetBound>,
    >,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for (entity, set, graph_ref, sources, state) in &unbound {
        if graph_ref.is_some_and(|graph_ref| !graph_ref.path.is_empty()) {
            continue;
        }
        let Some(sources) = sources else {
            commands.entity(entity).insert(AnimationSources(
                set.sources
                    .iter()
                    .map(|path| asset_server.load(path))
                    .collect(),
            ));
            continue;
        };
        let Some(loaded) = sources
            .0
            .iter()
            .map(|handle| gltfs.get(handle))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(&root) = descendants_named(entity, &set.skeleton_root, &children, &names).first()
        else {
            continue;
        };

        let mut graph = AnimationGraph::new();
        let mut nodes = HashMap::new();
        for def in &set.states {
            let Some(gltf) = loaded.get(def.source) else {
                warn!(
                    "animation state `{}` wants source {} of {}",
                    def.name,
                    def.source,
                    loaded.len()
                );
                continue;
            };
            let Some(clip) = gltf.named_animations.get(def.clip.as_str()) else {
                let available: Vec<&str> =
                    gltf.named_animations.keys().map(|name| &**name).collect();
                warn!(
                    "animation state `{}` wants clip `{}`, and its source holds {:?}",
                    def.name, def.clip, available
                );
                continue;
            };
            let node = graph.add_clip(clip.clone(), 1.0, graph.root);
            nodes.insert(def.name.clone(), node);
        }
        let graph = graphs.add(graph);

        commands.queue(move |world: &mut World| {
            tag_animation_targets(world, root);
        });

        let wanted = state.map_or(set.default_state.as_str(), |state| state.0.as_str());
        let mut player = AnimationPlayer::default();
        let mut transitions = AnimationTransitions::new();
        let mut warned_states = HashSet::new();
        match resolve_state(set, &nodes, wanted) {
            Some((def, node)) => play_state(def, node, &mut player, &mut transitions),
            None if wanted.is_empty() => {}
            None => {
                warn!("animation set has no state named `{wanted}`");
                warned_states.insert(wanted.to_string());
            }
        }
        commands
            .entity(root)
            .insert((player, AnimationGraphHandle(graph.clone()), transitions));

        if state.is_none() {
            commands
                .entity(entity)
                .insert(AnimationState(wanted.to_string()));
        }
        commands.entity(entity).insert(AnimationSetBound {
            player: root,
            nodes,
            graph,
            parts: Vec::new(),
            warned_states,
        });
    }
}

/// Folds a part exported with its own copy of the skeleton onto the skeleton
/// already bound, so one player drives the whole body.
///
/// A part whose bones do not answer to the same names keeps its own skeleton
/// and becomes a second animated root on the same graph, which costs a player
/// but leaves the part visible and in step.
fn merge_part_skeletons(
    mut commands: Commands,
    mut sets: Query<(
        Entity,
        &AnimationSet,
        &mut AnimationSetBound,
        Option<&AnimationState>,
    )>,
    children: Query<&Children>,
    names: Query<&Name>,
    child_of: Query<&ChildOf>,
    animated: Query<(), With<AnimationPlayer>>,
    mut skins: Query<&mut SkinnedMesh>,
) {
    for (entity, set, mut bound, state) in &mut sets {
        let primary = bound.player;
        let candidates: Vec<Entity> =
            descendants_named(entity, &set.skeleton_root, &children, &names)
                .into_iter()
                .filter(|&part| part != primary && !animated.contains(part))
                .collect();
        if candidates.is_empty() {
            continue;
        }

        let primary_joints: HashMap<String, Entity> = descendants(primary, &children)
            .into_iter()
            .filter_map(|joint| names.get(joint).ok().map(|name| (name.to_string(), joint)))
            .collect();
        let primary_parent = child_of
            .get(primary)
            .map_or(entity, |&ChildOf(parent)| parent);

        for part in candidates {
            let part_root = part_subtree_root(part, primary, &child_of, &children);
            let part_meshes: Vec<Entity> = descendants(part_root, &children)
                .into_iter()
                .filter(|&mesh| skins.contains(mesh))
                .collect();
            let Some(remapped) = remap_joints(&part_meshes, &primary_joints, &skins, &names) else {
                warn!(
                    "an animated part does not answer to the same bone names as the skeleton it \
                     was placed under, so it keeps its own"
                );
                bind_extra_root(&mut commands, part, &bound, set, state);
                bound.parts.push(part);
                continue;
            };
            for (mesh, joints) in remapped {
                if let Ok(mut skin) = skins.get_mut(mesh) {
                    skin.joints = joints;
                }
                commands.entity(mesh).insert(ChildOf(primary_parent));
            }
            commands.entity(part).despawn();
        }
    }
}

/// The subtree holding a part's meshes: a part exported beside its skeleton
/// keeps them as the skeleton's siblings, so its root is the part's parent.
fn part_subtree_root(
    part: Entity,
    primary: Entity,
    child_of: &Query<&ChildOf>,
    children: &Query<&Children>,
) -> Entity {
    child_of
        .get(part)
        .map(|&ChildOf(parent)| parent)
        .ok()
        .filter(|&parent| !descendants(parent, children).contains(&primary))
        .unwrap_or(part)
}

/// Plays the state an [`AnimationState`] was changed to.
fn apply_animation_state(
    mut sets: Query<
        (&AnimationSet, &mut AnimationSetBound, &AnimationState),
        Changed<AnimationState>,
    >,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (set, mut bound, state) in &mut sets {
        let Some((def, node)) = resolve_state(set, &bound.nodes, &state.0) else {
            if !state.0.is_empty() && bound.warned_states.insert(state.0.clone()) {
                warn!("animation set has no state named `{}`", state.0);
            }
            continue;
        };
        let roots: Vec<Entity> = std::iter::once(bound.player)
            .chain(bound.parts.iter().copied())
            .collect();
        for root in roots {
            let Ok((mut player, mut transitions)) = players.get_mut(root) else {
                continue;
            };
            play_state(def, node, &mut player, &mut transitions);
        }
    }
}

/// What an entity is playing, and where its playhead stands.
#[derive(Clone, Copy)]
struct PlayingClip<'a> {
    /// The file the clip came out of, as the set or graph names it.
    source: &'a str,
    /// The clip's name in that file.
    clip: &'a str,
    /// Seconds into the clip, as of the last tick.
    seek: f32,
    /// The way that tick moved through the clip.
    pass: ClipPass,
}

impl<'a> PlayingClip<'a> {
    /// Reads a clip's playhead and how it travelled off the animation playing
    /// it; a finished run reads as past the end, not as a wrap.
    fn read(source: &'a str, clip: &'a str, active: &ActiveAnimation) -> Self {
        let wrapped = active.just_completed() && !active.is_finished();
        Self {
            source,
            clip,
            seek: active.seek_time(),
            pass: match (active.is_playback_reversed(), wrapped) {
                (false, false) => ClipPass::Forward,
                (false, true) => ClipPass::ForwardWrapped,
                (true, false) => ClipPass::Backward,
                (true, true) => ClipPass::BackwardWrapped,
            },
        }
    }
}

/// Writes each bound entity's playhead onto the clip entity that carries that
/// clip's events; a graph beside a set drives the entity alone.
pub fn write_clip_playheads(
    mut commands: Commands,
    sets: Query<
        (Entity, &AnimationSet, &AnimationSetBound, &AnimationState),
        Without<AnimationGraphBound>,
    >,
    graphs: Query<(Entity, &AnimationGraphBound, &graph::AnimationGraphPlayback)>,
    players: Query<&AnimationPlayer>,
    installed: Query<&AnimationGraphHandle>,
    children: Query<&Children>,
    names: Query<&Name>,
    marked: Query<(), With<ClipEvent>>,
    mut playheads: Query<&mut ClipPlayhead>,
) {
    for (owner, set, bound, state) in &sets {
        if !plays_its_own_graph(bound.player, &bound.graph, &installed) {
            continue;
        }
        let playing = resolve_state(set, &bound.nodes, &state.0).and_then(|(def, node)| {
            let active = players.get(bound.player).ok()?.animation(node)?;
            let source = set.sources.get(def.source).map_or("", String::as_str);
            Some(PlayingClip::read(source, def.clip.as_str(), active))
        });
        write_playheads_under(
            owner,
            playing,
            &mut commands,
            &children,
            &names,
            &marked,
            &mut playheads,
        );
    }
    for (owner, bound, playback) in &graphs {
        if !plays_its_own_graph(bound.player, &bound.graph, &installed) {
            continue;
        }
        let playing = players.get(bound.player).ok().and_then(|player| {
            let (source, clip, node) = bound.leading_clip_of(player, &playback.state)?;
            Some(PlayingClip::read(source, clip, player.animation(node)?))
        });
        write_playheads_under(
            owner,
            playing,
            &mut commands,
            &children,
            &names,
            &marked,
            &mut playheads,
        );
    }
}

/// Whether a player is still wearing the graph its binding built, rather than
/// one an editor swapped in to preview a clip through it.
fn plays_its_own_graph(
    player: Entity,
    graph: &Handle<AnimationGraph>,
    installed: &Query<&AnimationGraphHandle>,
) -> bool {
    installed
        .get(player)
        .is_ok_and(|handle| handle.0.id() == graph.id())
}

/// Moves the playhead of the clip entity directly under `owner` that stands
/// for the playing clip, seeds one that just started, and clears the rest.
fn write_playheads_under(
    owner: Entity,
    playing: Option<PlayingClip<'_>>,
    commands: &mut Commands,
    children: &Query<&Children>,
    names: &Query<&Name>,
    marked: &Query<(), With<ClipEvent>>,
    playheads: &mut Query<&mut ClipPlayhead>,
) {
    let Ok(kids) = children.get(owner) else {
        return;
    };
    for row in kids.iter() {
        let holds_events = children
            .get(row)
            .is_ok_and(|keys| keys.iter().any(|key| marked.contains(key)));
        if !holds_events {
            continue;
        }
        let name = names.get(row).map_or("", Name::as_str);
        let played = playing.filter(|clip| stands_for_clip(name, clip.source, clip.clip));
        match (playheads.get_mut(row), played) {
            (Ok(mut playhead), Some(clip)) => playhead.advance_to(clip.seek, clip.pass),
            (Ok(_), None) => {
                commands.entity(row).remove::<ClipPlayhead>();
            }
            (Err(_), Some(clip)) => {
                commands.entity(row).insert(ClipPlayhead {
                    last: clip.seek,
                    now: clip.seek,
                    pass: ClipPass::Forward,
                });
            }
            (Err(_), None) => {}
        }
    }
}

/// Whether a clip entity name stands for `clip` out of `source`: a bare
/// clip name, or `<file>#<clip>` when the file matters.
pub fn stands_for_clip(name: &str, source: &str, clip: &str) -> bool {
    match name.split_once('#') {
        Some((file, named)) => file == source && named == clip,
        None => name == clip,
    }
}

/// Reports a non-looping state that has run out, and moves on to whatever it
/// said should follow.
fn report_finished_states(
    mut sets: Query<(
        Entity,
        &AnimationSet,
        &AnimationSetBound,
        &mut AnimationState,
    )>,
    players: Query<(&AnimationPlayer, &AnimationTransitions)>,
    mut finished: MessageWriter<AnimationStateFinished>,
) {
    for (entity, set, bound, mut state) in &mut sets {
        let Some((def, node)) = resolve_state(set, &bound.nodes, &state.0) else {
            continue;
        };
        if def.looped {
            continue;
        }
        let Ok((player, transitions)) = players.get(bound.player) else {
            continue;
        };
        if transitions.get_main_animation() != Some(node) {
            continue;
        }
        let ran_out_this_frame = player
            .animation(node)
            .is_some_and(|active| active.just_completed() && active.is_finished());
        if !ran_out_this_frame {
            continue;
        }
        finished.write(AnimationStateFinished {
            entity,
            state: state.0.clone(),
        });
        if let Some(next) = &def.then {
            state.0 = next.clone();
        }
    }
}

/// The state definition and graph node a name asks for, when the set has both.
fn resolve_state<'a>(
    set: &'a AnimationSet,
    nodes: &HashMap<String, AnimationNodeIndex>,
    wanted: &str,
) -> Option<(&'a AnimationStateDef, AnimationNodeIndex)> {
    let def = set.states.iter().find(|def| def.name == wanted)?;
    let node = *nodes.get(wanted)?;
    Some((def, node))
}

/// Starts a state on one animated root, fading out whatever it replaces.
fn play_state(
    def: &AnimationStateDef,
    node: AnimationNodeIndex,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
) {
    let already_running = transitions.get_main_animation() == Some(node)
        && player
            .animation(node)
            .is_some_and(|active| !active.is_finished());
    if already_running {
        return;
    }
    let repeat = if def.looped {
        RepeatAnimation::Forever
    } else {
        RepeatAnimation::Never
    };
    transitions
        .play(player, node, Duration::from_secs_f32(def.transition_secs))
        .set_repeat(repeat)
        .set_speed(def.speed);
}

/// Gives a part that kept its own skeleton a player of its own on the shared
/// graph, so it stays in step with the body it was placed under.
fn bind_extra_root(
    commands: &mut Commands,
    root: Entity,
    bound: &AnimationSetBound,
    set: &AnimationSet,
    state: Option<&AnimationState>,
) {
    commands.queue(move |world: &mut World| {
        tag_animation_targets(world, root);
    });
    let wanted = state.map_or(set.default_state.as_str(), |state| state.0.as_str());
    let mut player = AnimationPlayer::default();
    let mut transitions = AnimationTransitions::new();
    if let Some((def, node)) = resolve_state(set, &bound.nodes, wanted) {
        play_state(def, node, &mut player, &mut transitions);
    }
    commands.entity(root).insert((
        player,
        AnimationGraphHandle(bound.graph.clone()),
        transitions,
    ));
}

/// The joints each of a part's meshes should point at once it wears the
/// primary skeleton, or `None` when a bone name has no counterpart there.
fn remap_joints(
    meshes: &[Entity],
    primary_joints: &HashMap<String, Entity>,
    skins: &Query<&mut SkinnedMesh>,
    names: &Query<&Name>,
) -> Option<Vec<(Entity, Vec<Entity>)>> {
    let mut remapped = Vec::with_capacity(meshes.len());
    for &mesh in meshes {
        let skin = skins.get(mesh).ok()?;
        let joints = skin
            .joints
            .iter()
            .map(|&joint| primary_joints.get(names.get(joint).ok()?.as_str()).copied())
            .collect::<Option<Vec<_>>>()?;
        remapped.push((mesh, joints));
    }
    Some(remapped)
}

/// Gives every named entity under `root` the target id of its name path, so a
/// clip authored against a skeleton of the same names drives this one, and
/// reports the entities that had none.
///
/// An unnamed entity ends the walk: its descendants have no path to hash, and
/// glTF's own loader passes over them for the same reason.
///
/// Public because the editor tags a skeleton the same way to preview a clip on
/// it, and untags exactly what it was handed back when the preview stops.
pub fn tag_animation_targets(world: &mut World, root: Entity) -> Vec<Entity> {
    let mut tagged = Vec::new();
    tag_from(world, root, root, &mut Vec::new(), &mut tagged);
    tagged
}

fn tag_from(
    world: &mut World,
    root: Entity,
    entity: Entity,
    path: &mut Vec<Name>,
    tagged: &mut Vec<Entity>,
) {
    let Some(name) = world.get::<Name>(entity).cloned() else {
        return;
    };
    path.push(name);
    if world.get::<AnimationTargetId>(entity).is_none() {
        world
            .entity_mut(entity)
            .insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(root)));
        tagged.push(entity);
    }
    let kids: Vec<Entity> = world
        .get::<Children>(entity)
        .map(|kids| kids.iter().collect())
        .unwrap_or_default();
    for child in kids {
        tag_from(world, root, child, path, tagged);
    }
    path.pop();
}

/// Every descendant of `root` carrying `wanted` as its name, nearest first.
pub(crate) fn descendants_named(
    root: Entity,
    wanted: &str,
    children: &Query<&Children>,
    names: &Query<&Name>,
) -> Vec<Entity> {
    descendants(root, children)
        .into_iter()
        .skip(1)
        .filter(|&entity| names.get(entity).is_ok_and(|name| name.as_str() == wanted))
        .collect()
}

/// `root` and everything under it, breadth first.
fn descendants(root: Entity, children: &Query<&Children>) -> Vec<Entity> {
    let mut found = vec![root];
    let mut visited = 0;
    while visited < found.len() {
        let entity = found[visited];
        visited += 1;
        if let Ok(kids) = children.get(entity) {
            found.extend(kids.iter());
        }
    }
    found
}
