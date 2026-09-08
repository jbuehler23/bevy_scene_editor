//! An authored animation set finds the skeleton spawned under it, drives it
//! with clips exported against a skeleton of the same bone names, and plays
//! whichever state it is asked for.

use std::time::Duration;

use bevy::{
    animation::{
        AnimatedBy, AnimationClip, AnimationTargetId, animated_field,
        animation_curves::{AnimatableCurve, AnimatableKeyframeCurve},
        graph::AnimationNodeIndex,
        transition::AnimationTransitions,
    },
    asset::AssetPlugin,
    gltf::Gltf,
    mesh::skinning::SkinnedMesh,
    platform::collections::HashMap,
    prelude::*,
    time::TimeUpdateStrategy,
};
use jackdaw_animation_runtime::{
    AnimationEvent, AnimationRuntimePlugin, AnimationSet, AnimationSetBound, AnimationSetSystems,
    AnimationSources, AnimationState, AnimationStateDef, AnimationStateFinished, ClipEvent,
};

/// Every state reported finished so far.
#[derive(Resource, Default)]
struct Finished(Vec<String>);

/// Every clip event sent so far, with the entity it named.
#[derive(Resource, Default)]
struct Fired(Vec<(Entity, String)>);

fn animation_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(bevy::transform::TransformPlugin)
        .add_plugins(bevy::animation::AnimationPlugin)
        .add_plugins(AnimationRuntimePlugin);
    app.init_asset::<Gltf>();
    app.init_resource::<Finished>();
    app.init_resource::<Fired>();
    app.add_systems(
        Update,
        (collect_finished, collect_fired).after(AnimationSetSystems),
    );
    app
}

fn collect_fired(mut sent: MessageReader<AnimationEvent>, mut out: ResMut<Fired>) {
    out.0
        .extend(sent.read().map(|event| (event.entity, event.name.clone())));
}

/// Puts a named moment on a clip row under `owner`: a child named for the clip
/// the events belong to, carrying one event.
fn clip_marker(app: &mut App, owner: Entity, clip: &str, time: f32, name: &str) {
    let row = app
        .world_mut()
        .spawn((Name::new(clip.to_string()), ChildOf(owner)))
        .id();
    app.world_mut().spawn((
        ClipEvent {
            time,
            name: name.to_string(),
        },
        ChildOf(row),
    ));
}

fn fired_names(app: &App) -> Vec<String> {
    app.world()
        .resource::<Fired>()
        .0
        .iter()
        .map(|(_, name)| name.clone())
        .collect()
}

fn collect_finished(
    mut reported: MessageReader<AnimationStateFinished>,
    mut out: ResMut<Finished>,
) {
    out.0
        .extend(reported.read().map(|finished| finished.state.clone()));
}

/// Runs one frame of a length the test picks rather than the wall clock's.
fn step(app: &mut App, delta: Duration) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(delta));
    app.update();
}

fn millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Spawns a set on a root, with `bones` hanging off an armature inside a scene
/// instance of its own, the way a loaded glTF body arrives.
fn spawn_rig(app: &mut App, set: AnimationSet, instance: &str, bones: &[&str]) -> Entity {
    let armature_name = set.skeleton_root.clone();
    let root = app.world_mut().spawn(set).id();
    spawn_body(app, root, instance, &armature_name, bones);
    root
}

/// Spawns one scene instance under `root`: an armature and a chain of bones.
fn spawn_body(
    app: &mut App,
    root: Entity,
    instance: &str,
    armature_name: &str,
    bones: &[&str],
) -> Entity {
    let instance = app
        .world_mut()
        .spawn((Name::new(instance.to_string()), ChildOf(root)))
        .id();
    let mut parent = app
        .world_mut()
        .spawn((
            Name::new(armature_name.to_string()),
            Transform::default(),
            ChildOf(instance),
        ))
        .id();
    for bone in bones {
        parent = app
            .world_mut()
            .spawn((
                Name::new(bone.to_string()),
                Transform::default(),
                ChildOf(parent),
            ))
            .id();
    }
    instance
}

/// Spawns named nodes under `parent`, the way a glTF file's mesh nodes sit
/// among the bones of the armature they are skinned to.
fn spawn_mesh_nodes(app: &mut App, parent: Entity, names: &[&str]) {
    for name in names {
        app.world_mut().spawn((
            Name::new((*name).to_string()),
            Transform::default(),
            ChildOf(parent),
        ));
    }
}

/// A clip that slides the bone at `path` one unit along X over a second.
fn sliding_clip(app: &mut App, path: &[&str]) -> Handle<AnimationClip> {
    sliding_clip_lasting(app, path, 1.0)
}

/// The same slide over a length the test picks.
fn sliding_clip_lasting(app: &mut App, path: &[&str], seconds: f32) -> Handle<AnimationClip> {
    let names: Vec<Name> = path
        .iter()
        .map(|name| Name::new(name.to_string()))
        .collect();
    let curve = AnimatableKeyframeCurve::new([(0.0, Vec3::ZERO), (seconds, Vec3::X)])
        .expect("two keyframes make a curve");
    let mut clip = AnimationClip::default();
    clip.add_curve_to_target(
        AnimationTargetId::from_names(names.iter()),
        AnimatableCurve::new(animated_field!(Transform::translation), curve),
    );
    app.world_mut()
        .resource_mut::<Assets<AnimationClip>>()
        .add(clip)
}

/// A stand-in for a loaded glTF file that holds nothing but named clips.
fn source_holding(app: &mut App, clips: &[(&str, Handle<AnimationClip>)]) -> Handle<Gltf> {
    let gltf = Gltf {
        scenes: Vec::new(),
        named_scenes: HashMap::default(),
        meshes: Vec::new(),
        named_meshes: HashMap::default(),
        materials: Vec::new(),
        named_materials: HashMap::default(),
        nodes: Vec::new(),
        named_nodes: HashMap::default(),
        skins: Vec::new(),
        named_skins: HashMap::default(),
        default_scene: None,
        animations: clips.iter().map(|(_, clip)| clip.clone()).collect(),
        named_animations: clips
            .iter()
            .map(|(name, clip)| ((*name).into(), clip.clone()))
            .collect(),
        source: None,
    };
    app.world_mut().resource_mut::<Assets<Gltf>>().add(gltf)
}

fn named(app: &mut App, wanted: &str) -> Entity {
    let mut names = app.world_mut().query::<(Entity, &Name)>();
    names
        .iter(app.world())
        .find_map(|(entity, name)| (name.as_str() == wanted).then_some(entity))
        .unwrap_or_else(|| panic!("expected an entity named {wanted}"))
}

fn target_of(app: &App, entity: Entity) -> AnimationTargetId {
    *app.world()
        .get::<AnimationTargetId>(entity)
        .expect("a bound bone carries a target id")
}

fn node_for(app: &App, root: Entity, state: &str) -> AnimationNodeIndex {
    app.world()
        .get::<AnimationSetBound>(root)
        .expect("the set is bound")
        .nodes[state]
}

fn walking_set(
    sources: Vec<String>,
    states: Vec<AnimationStateDef>,
    default_state: &str,
) -> AnimationSet {
    AnimationSet {
        sources,
        states,
        default_state: default_state.to_string(),
        ..AnimationSet::default()
    }
}

#[test]
fn a_bound_set_gives_every_bone_the_target_id_of_its_name_path() {
    let mut app = animation_app();
    spawn_rig(
        &mut app,
        AnimationSet::default(),
        "Body",
        &["Hips", "Spine"],
    );

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);

    let armature = named(&mut app, "Armature");
    let hips = named(&mut app, "Hips");
    let spine = named(&mut app, "Spine");
    let path = |names: &[&str]| {
        let names: Vec<Name> = names.iter().map(|n| Name::new(n.to_string())).collect();
        AnimationTargetId::from_names(names.iter())
    };

    assert_eq!(target_of(&app, armature), path(&["Armature"]));
    assert_eq!(target_of(&app, hips), path(&["Armature", "Hips"]));
    assert_eq!(target_of(&app, spine), path(&["Armature", "Hips", "Spine"]));
    assert_eq!(
        app.world().get::<AnimatedBy>(spine).map(|by| by.0),
        Some(armature),
        "every bone is animated by the skeleton root, not by whatever spawned it"
    );
}

#[test]
fn a_clip_authored_against_one_skeleton_moves_the_same_named_bone_on_another() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));

    for _ in 0..5 {
        step(&mut app, millis(100));
    }

    let hips = named(&mut app, "Hips");
    let moved = app.world().get::<Transform>(hips).unwrap().translation.x;
    assert!(
        moved > 0.0,
        "the clip names bones by path, so it drives a skeleton it was never exported with: {moved}"
    );
}

#[test]
fn setting_a_state_plays_its_clip_and_fades_the_previous_one_over_the_transition() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Idle", clip.clone()), ("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![
            AnimationStateDef {
                name: "idle".to_string(),
                clip: "Idle".to_string(),
                ..AnimationStateDef::default()
            },
            AnimationStateDef {
                name: "walk".to_string(),
                clip: "Walk".to_string(),
                transition_secs: 0.5,
                ..AnimationStateDef::default()
            },
        ],
        "idle",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    step(&mut app, millis(100));

    let armature = named(&mut app, "Armature");
    let idle = node_for(&app, root, "idle");
    let walk = node_for(&app, root, "walk");
    assert_eq!(
        app.world()
            .get::<AnimationTransitions>(armature)
            .unwrap()
            .get_main_animation(),
        Some(idle),
        "the default state is what a freshly bound set plays"
    );

    app.world_mut()
        .entity_mut(root)
        .insert(AnimationState("walk".to_string()));
    step(&mut app, millis(100));

    assert_eq!(
        app.world()
            .get::<AnimationTransitions>(armature)
            .unwrap()
            .get_main_animation(),
        Some(walk),
    );
    let fading = app
        .world()
        .get::<AnimationPlayer>(armature)
        .unwrap()
        .animation(idle)
        .expect("the state being replaced is still playing while it fades")
        .weight();
    assert!(
        (0.0..1.0).contains(&fading),
        "a fifth of the way through the transition the old state is part-weight: {fading}"
    );

    for _ in 0..6 {
        step(&mut app, millis(100));
    }
    assert!(
        app.world()
            .get::<AnimationPlayer>(armature)
            .unwrap()
            .animation(idle)
            .is_none(),
        "once the transition is over the state it replaced has stopped"
    );
}

#[test]
fn a_one_shot_state_reports_finished_and_falls_back_to_its_then_state() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Attack", clip.clone()), ("Idle", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![
            AnimationStateDef {
                name: "attack".to_string(),
                clip: "Attack".to_string(),
                looped: false,
                then: Some("idle".to_string()),
                ..AnimationStateDef::default()
            },
            AnimationStateDef {
                name: "idle".to_string(),
                clip: "Idle".to_string(),
                ..AnimationStateDef::default()
            },
        ],
        "attack",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));

    for _ in 0..10 {
        step(&mut app, millis(200));
    }

    assert_eq!(
        app.world().resource::<Finished>().0,
        vec!["attack".to_string()],
        "a one-shot state reports once, on the frame its clip ran out"
    );
    assert_eq!(
        app.world().get::<AnimationState>(root),
        Some(&AnimationState("idle".to_string())),
        "and the set moves on to what the state said should follow"
    );
    let armature = named(&mut app, "Armature");
    assert_eq!(
        app.world()
            .get::<AnimationTransitions>(armature)
            .unwrap()
            .get_main_animation(),
        Some(node_for(&app, root, "idle")),
    );
}

#[test]
fn two_parts_sharing_a_rig_are_driven_by_one_skeleton_and_their_joints_move_identically() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Torso", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    let torso = named(&mut app, "Torso");
    let torso_armature = named(&mut app, "Armature");
    let torso_hips = named(&mut app, "Hips");
    let torso_mesh = app
        .world_mut()
        .spawn((
            Name::new("TorsoMesh"),
            SkinnedMesh {
                joints: vec![torso_armature, torso_hips],
                ..SkinnedMesh::default()
            },
            ChildOf(torso),
        ))
        .id();

    let legs = spawn_body(&mut app, root, "Legs", "Armature", &["Hips"]);
    let legs_armature = app.world().get::<Children>(legs).unwrap()[0];
    let legs_hips = app.world().get::<Children>(legs_armature).unwrap()[0];
    let legs_mesh = app
        .world_mut()
        .spawn((
            Name::new("LegsMesh"),
            SkinnedMesh {
                joints: vec![legs_armature, legs_hips],
                ..SkinnedMesh::default()
            },
            ChildOf(legs),
        ))
        .id();

    for _ in 0..5 {
        step(&mut app, millis(100));
    }

    let torso_joints = app
        .world()
        .get::<SkinnedMesh>(torso_mesh)
        .unwrap()
        .joints
        .clone();
    let legs_joints = app
        .world()
        .get::<SkinnedMesh>(legs_mesh)
        .unwrap()
        .joints
        .clone();
    assert_eq!(
        legs_joints, torso_joints,
        "the part was re-pointed at the bones the other part already uses"
    );
    assert!(
        app.world().get_entity(legs_armature).is_err(),
        "the part's own copy of the skeleton is gone"
    );
    assert_eq!(
        app.world().get::<ChildOf>(legs_mesh).map(|parent| parent.0),
        Some(torso),
        "and its mesh now hangs beside the skeleton driving it"
    );
    assert!(
        app.world()
            .get::<Transform>(torso_hips)
            .unwrap()
            .translation
            .x
            > 0.0,
        "one player moves the bones both meshes are skinned to"
    );
}

#[test]
fn a_part_merges_when_its_joints_match_even_if_mesh_node_counts_differ() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Torso", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    let torso = named(&mut app, "Torso");
    let torso_armature = app.world().get::<Children>(torso).unwrap()[0];
    let torso_hips = app.world().get::<Children>(torso_armature).unwrap()[0];
    spawn_mesh_nodes(
        &mut app,
        torso_armature,
        &["TorsoSkin", "TorsoTrim", "TorsoBelt"],
    );
    app.world_mut().spawn((
        Name::new("TorsoMesh"),
        SkinnedMesh {
            joints: vec![torso_armature, torso_hips],
            ..SkinnedMesh::default()
        },
        ChildOf(torso),
    ));

    let legs = spawn_body(&mut app, root, "Legs", "Armature", &["Hips"]);
    let legs_armature = app.world().get::<Children>(legs).unwrap()[0];
    let legs_hips = app.world().get::<Children>(legs_armature).unwrap()[0];
    spawn_mesh_nodes(&mut app, legs_armature, &["LegsSkin"]);
    let legs_mesh = app
        .world_mut()
        .spawn((
            Name::new("LegsMesh"),
            SkinnedMesh {
                joints: vec![legs_armature, legs_hips],
                ..SkinnedMesh::default()
            },
            ChildOf(legs),
        ))
        .id();

    for _ in 0..5 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        app.world()
            .get::<SkinnedMesh>(legs_mesh)
            .unwrap()
            .joints
            .clone(),
        vec![torso_armature, torso_hips],
        "the bones a part's mesh names are what decides the merge, not how many mesh nodes each \
         file carries"
    );
    assert!(
        app.world().get_entity(legs_armature).is_err(),
        "the part's own copy of the skeleton is gone"
    );
    assert!(
        app.world()
            .get::<AnimationSetBound>(root)
            .unwrap()
            .parts
            .is_empty(),
        "and no second player was needed"
    );
}

#[test]
fn a_part_whose_joints_do_not_match_by_name_keeps_its_own_skeleton_and_warns_once() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Torso", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));

    let cloak = spawn_body(&mut app, root, "Cloak", "Armature", &["Tail"]);
    let cloak_armature = app.world().get::<Children>(cloak).unwrap()[0];
    let cloak_tail = app.world().get::<Children>(cloak_armature).unwrap()[0];
    let cloak_mesh = app
        .world_mut()
        .spawn((
            Name::new("CloakMesh"),
            SkinnedMesh {
                joints: vec![cloak_armature, cloak_tail],
                ..SkinnedMesh::default()
            },
            ChildOf(cloak),
        ))
        .id();

    for _ in 0..5 {
        step(&mut app, millis(100));
    }

    let bound = app.world().get::<AnimationSetBound>(root).unwrap();
    assert_eq!(
        bound.parts,
        vec![cloak_armature],
        "the part is taken up once and then left alone"
    );
    assert_eq!(
        app.world()
            .get::<SkinnedMesh>(cloak_mesh)
            .unwrap()
            .joints
            .clone(),
        vec![cloak_armature, cloak_tail],
        "a part whose bones answer to other names keeps the ones it came with"
    );
    assert!(
        app.world().get::<AnimationPlayer>(cloak_armature).is_some(),
        "and gets a player of its own so it still plays the set's state"
    );
}

#[test]
fn a_set_whose_skeleton_has_not_spawned_yet_binds_once_it_appears() {
    let mut app = animation_app();
    let root = app.world_mut().spawn(AnimationSet::default()).id();

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    assert!(
        app.world().get::<AnimationSetBound>(root).is_none(),
        "nothing binds while the skeleton the set names is still loading"
    );

    spawn_body(&mut app, root, "Body", "Armature", &["Hips"]);
    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);

    let armature = named(&mut app, "Armature");
    assert_eq!(
        app.world().get::<AnimationSetBound>(root).map(|b| b.player),
        Some(armature),
        "the set binds on the frame its skeleton turns up"
    );
    assert!(app.world().get::<AnimationPlayer>(armature).is_some());
}

#[test]
fn an_unknown_state_name_is_refused_with_one_warning() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Idle", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "idle".to_string(),
            clip: "Idle".to_string(),
            ..AnimationStateDef::default()
        }],
        "idle",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    step(&mut app, millis(100));
    let idle = node_for(&app, root, "idle");

    for _ in 0..2 {
        app.world_mut()
            .entity_mut(root)
            .insert(AnimationState("sprint".to_string()));
        step(&mut app, millis(100));
    }

    let bound = app.world().get::<AnimationSetBound>(root).unwrap();
    assert_eq!(
        bound.warned_states.iter().collect::<Vec<_>>(),
        vec!["sprint"],
        "asking twice for a state the set has never heard of is reported once"
    );
    let armature = named(&mut app, "Armature");
    assert_eq!(
        app.world()
            .get::<AnimationTransitions>(armature)
            .unwrap()
            .get_main_animation(),
        Some(idle),
        "and the state that was playing keeps playing"
    );
}

#[test]
fn a_marker_fires_once_on_every_pass_of_a_looping_clip() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Walk", 0.4, "step");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..30 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        app.world().resource::<Fired>().0,
        vec![(root, "step".to_string()); 3],
        "a second-long clip run for three seconds should send its 0.4s marker \
         once on each pass, named against the entity the clip animates"
    );
}

#[test]
fn a_one_shot_state_fires_its_marker_once_and_says_nothing_after_it_ends() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Hit", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "hit".to_string(),
            clip: "Hit".to_string(),
            looped: false,
            ..AnimationStateDef::default()
        }],
        "hit",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Hit", 0.4, "impact");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..25 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        fired_names(&app),
        vec!["impact".to_string()],
        "a clip that runs once holds its last frame, and a marker already \
         passed must not send again while it sits there"
    );
}

#[test]
fn a_state_left_before_its_marker_never_fires_it() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Idle", clip.clone()), ("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![
            AnimationStateDef {
                name: "idle".to_string(),
                clip: "Idle".to_string(),
                ..AnimationStateDef::default()
            },
            AnimationStateDef {
                name: "walk".to_string(),
                clip: "Walk".to_string(),
                ..AnimationStateDef::default()
            },
        ],
        "idle",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Idle", 0.4, "blink");
    clip_marker(&mut app, root, "Walk", 0.4, "stride");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..4 {
        step(&mut app, millis(100));
    }
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationState("walk".to_string()));
    for _ in 0..8 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        fired_names(&app),
        vec!["stride".to_string()],
        "leaving idle at 0.3s drops its 0.4s marker rather than sending it \
         late, while the state moved to sends its own"
    );
}

#[test]
fn a_marker_at_the_head_of_a_clip_fires_on_every_pass_of_it() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Walk", 0.0, "plant");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..25 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        fired_names(&app),
        vec!["plant".to_string(); 3],
        "a marker sitting on the first frame is passed once on the way out of \
         it and once on every pass after"
    );
}

#[test]
fn a_state_played_backwards_sends_each_marker_once_a_pass() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            speed: -1.0,
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Walk", 0.4, "step");
    clip_marker(&mut app, root, "Walk", 0.7, "land");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..25 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        fired_names(&app),
        ["land", "step", "land", "step", "land"].map(str::to_string),
        "a clip walked backwards passes each marker once a lap, in the order \
         it meets them, rather than a whole clip of them at the wrap"
    );
}

#[test]
fn a_clip_shorter_than_the_frame_running_it_still_fires_its_marker() {
    let mut app = animation_app();
    let clip = sliding_clip_lasting(&mut app, &["Armature", "Hips"], 0.1);
    let source = source_holding(&mut app, &[("Flash", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "flash".to_string(),
            clip: "Flash".to_string(),
            ..AnimationStateDef::default()
        }],
        "flash",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Flash", 0.08, "tick");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..6 {
        step(&mut app, millis(200));
    }

    let fired = fired_names(&app);
    assert_eq!(
        fired,
        vec!["tick".to_string(); 5],
        "a frame that laps a clip more than once leaves the playhead where it \
         found it, and the marker it swept past must still send: {fired:?}"
    );
}

#[test]
fn a_paused_player_sends_nothing_until_it_is_let_go() {
    let mut app = animation_app();
    let clip = sliding_clip(&mut app, &["Armature", "Hips"]);
    let source = source_holding(&mut app, &[("Walk", clip)]);
    let set = walking_set(
        vec!["rig.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![source]));
    clip_marker(&mut app, root, "Walk", 0.4, "step");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    let node = node_for(&app, root, "walk");
    let armature = named(&mut app, "Armature");
    app.world_mut()
        .get_mut::<AnimationPlayer>(armature)
        .expect("the skeleton carries a player")
        .animation_mut(node)
        .expect("the state is playing")
        .pause();
    for _ in 0..20 {
        step(&mut app, millis(100));
    }

    assert!(
        fired_names(&app).is_empty(),
        "a playhead that is not moving passes nothing: {:?}",
        fired_names(&app)
    );

    app.world_mut()
        .get_mut::<AnimationPlayer>(armature)
        .expect("the skeleton carries a player")
        .animation_mut(node)
        .expect("the state is playing")
        .resume();
    for _ in 0..6 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        fired_names(&app),
        vec!["step".to_string()],
        "and it sends again once playback moves on"
    );
}

#[test]
fn a_marker_row_naming_its_source_file_fires_only_for_that_source() {
    let mut app = animation_app();
    let first = sliding_clip(&mut app, &["Armature", "Hips"]);
    let second = sliding_clip(&mut app, &["Armature", "Hips"]);
    let one = source_holding(&mut app, &[("Walk", first)]);
    let two = source_holding(&mut app, &[("Walk", second)]);
    let set = walking_set(
        vec!["one.glb".to_string(), "two.glb".to_string()],
        vec![AnimationStateDef {
            name: "walk".to_string(),
            source: 1,
            clip: "Walk".to_string(),
            ..AnimationStateDef::default()
        }],
        "walk",
    );
    let root = spawn_rig(&mut app, set, "Body", &["Hips"]);
    app.world_mut()
        .entity_mut(root)
        .insert(AnimationSources(vec![one, two]));
    clip_marker(&mut app, root, "one.glb#Walk", 0.4, "wrong file");
    clip_marker(&mut app, root, "two.glb#Walk", 0.4, "right file");

    step(&mut app, Duration::ZERO);
    step(&mut app, Duration::ZERO);
    for _ in 0..8 {
        step(&mut app, millis(100));
    }

    assert_eq!(
        fired_names(&app),
        vec!["right file".to_string()],
        "two files holding a clip of the same name are told apart by a row \
         named `<file>#<clip>`"
    );
}
