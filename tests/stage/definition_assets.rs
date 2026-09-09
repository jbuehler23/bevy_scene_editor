//! Definition assets: a registered asset type edited as files in the project.
//!
//! A definition type claims a directory and a suffix, and its files are
//! created, edited, saved and listed through the same operators whether the
//! edit comes from the inspector or from a caller with no viewport.

use crate::util;

use bevy::asset::{Asset, Assets, UntypedHandle};
use bevy::prelude::*;
use jackdaw::definition_assets::{
    DefinitionAssetEdit, DefinitionRegistry, MATERIAL_KIND, OpenDefinition,
};
use jackdaw_api::prelude::*;
use jackdaw_api_internal::operator::{CallOperatorSettings, ExecutionContext};
use jackdaw_commands::CommandHistory;
use jackdaw_scene_types::PropertyValue;

#[derive(Reflect, Clone, Default, PartialEq, Debug)]
#[reflect(Default)]
struct LootRoll {
    item: String,
    weight: u32,
}

#[derive(Reflect, Clone, Default, PartialEq, Debug)]
#[reflect(Default)]
enum Rarity {
    #[default]
    Common,
    Rare,
}

#[derive(Asset, Reflect, Clone, Default)]
#[reflect(Default)]
struct ItemDef {
    stack_size: u32,
    rarity: Rarity,
    loot: Vec<LootRoll>,
}

fn item_type() -> DefinitionAssetType {
    DefinitionAssetType::new(
        "item",
        "Item",
        ItemDef::type_path(),
        "content/items",
        ".item.bsn",
    )
}

/// An editor with a project of its own and one definition type registered.
fn editor_with_items() -> (App, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut app = util::editor_test_app();
    app.init_asset::<ItemDef>();
    app.register_asset_reflect::<ItemDef>();
    app.register_type::<ItemDef>();
    app.register_type::<LootRoll>();
    app.register_type::<Rarity>();
    app.world_mut()
        .insert_resource(jackdaw::project::ProjectRoot {
            root: tmp.path().to_path_buf(),
            config: default(),
        });
    app.world_mut()
        .resource_mut::<DefinitionAssetTypes>()
        .register(item_type());
    app.world_mut()
        .resource_mut::<NextState<jackdaw::AppState>>()
        .set(jackdaw::AppState::Editor);
    app.update();
    (app, tmp)
}

#[track_caller]
fn call(app: &mut App, id: &'static str, params: &[(&'static str, PropertyValue)]) {
    let mut call = app.world_mut().operator(id).settings(CallOperatorSettings {
        execution_context: ExecutionContext::Invoke,
        creates_history_entry: true,
    });
    for (key, value) in params {
        call = call.param(*key, value.clone());
    }
    let result = call.call().expect("the operator dispatched");
    assert_eq!(result, OperatorResult::Finished, "{id} did not finish");
    app.update();
}

fn open_item(app: &App) -> ItemDef {
    let entity = app
        .world()
        .resource::<OpenDefinition>()
        .0
        .expect("a definition is open");
    let handle = app
        .world()
        .get::<DefinitionAssetEdit>(entity)
        .expect("the entity is editing a definition")
        .handle
        .clone();
    item_of(app, &handle)
}

fn item_of(app: &App, handle: &UntypedHandle) -> ItemDef {
    app.world()
        .resource::<Assets<ItemDef>>()
        .get(&handle.clone().typed::<ItemDef>())
        .expect("the definition is in its store")
        .clone()
}

#[test]
fn a_definition_is_created_edited_and_saved_through_its_operators() {
    let (mut app, tmp) = editor_with_items();

    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );
    let path = tmp.path().join("assets/content/items/torch.item.bsn");
    assert!(path.is_file(), "asset.new writes the file at {path:?}");

    call(
        &mut app,
        "asset.set",
        &[("field", "stack_size".into()), ("value", "12".into())],
    );
    call(
        &mut app,
        "asset.set",
        &[("field", "rarity".into()), ("value", "Rare".into())],
    );
    call(
        &mut app,
        "asset.set",
        &[
            ("field", "loot".into()),
            ("value", r#"[{"item":"coin","weight":3}]"#.into()),
        ],
    );

    let edited = open_item(&app);
    assert_eq!(edited.stack_size, 12);
    assert_eq!(edited.rarity, Rarity::Rare);
    assert_eq!(
        edited.loot,
        vec![LootRoll {
            item: "coin".into(),
            weight: 3
        }]
    );

    call(&mut app, "asset.save", &[]);
    let written = std::fs::read_to_string(&path).expect("the file reads");
    assert!(written.contains("stack_size: 12"), "got:\n{written}");
    assert!(written.contains("Rare"), "got:\n{written}");
    assert!(written.contains("coin"), "got:\n{written}");

    app.world_mut()
        .resource_mut::<DefinitionRegistry>()
        .remove("item", "torch");
    let scan = jackdaw::definition_assets::rescan_definitions(app.world_mut());
    assert_eq!(scan.added, vec![("item".to_string(), "torch".to_string())]);
    let reloaded = {
        let handle = app
            .world()
            .resource::<DefinitionRegistry>()
            .get("item", "torch")
            .expect("the scan found it")
            .handle
            .clone();
        item_of(&app, &handle)
    };
    assert_eq!(reloaded.stack_size, 12);
    assert_eq!(reloaded.rarity, Rarity::Rare);
    assert_eq!(reloaded.loot.len(), 1);
}

#[test]
fn undo_takes_back_one_definition_field_edit() {
    let (mut app, _tmp) = editor_with_items();
    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );
    call(
        &mut app,
        "asset.set",
        &[("field", "stack_size".into()), ("value", "12".into())],
    );
    assert_eq!(open_item(&app).stack_size, 12);

    app.world_mut()
        .resource_scope(|world, mut history: Mut<CommandHistory>| {
            history.undo(world);
        });

    assert_eq!(
        open_item(&app).stack_size,
        0,
        "undo restores what the field held before the edit"
    );
}

#[test]
fn undo_reaches_the_definition_after_its_card_is_closed() {
    let (mut app, _tmp) = editor_with_items();
    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );
    call(
        &mut app,
        "asset.set",
        &[("field", "stack_size".into()), ("value", "12".into())],
    );
    let handle = app
        .world()
        .resource::<DefinitionRegistry>()
        .get("item", "torch")
        .expect("the definition is registered")
        .handle
        .clone();

    jackdaw::definition_assets::close_open_definition(app.world_mut());
    app.update();
    assert!(app.world().resource::<OpenDefinition>().0.is_none());

    app.world_mut()
        .resource_scope(|world, mut history: Mut<CommandHistory>| {
            history.undo(world);
        });

    assert_eq!(
        item_of(&app, &handle).stack_size,
        0,
        "the entry is keyed by handle, so it outlives the card"
    );
}

#[test]
fn a_scene_entity_edit_still_lands_while_a_definition_is_open() {
    let (mut app, _tmp) = editor_with_items();
    let node = app
        .world_mut()
        .spawn((Name::new("Target"), Node::default()))
        .id();
    jackdaw::scene_io::register_entity_in_ast(app.world_mut(), node);
    app.update();

    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );

    let result = app
        .world_mut()
        .operator("field.set")
        .param("entity", node)
        .param("type_path", Node::type_path().to_string())
        .param("field", "width".to_string())
        .param("value", "{\"Px\":120.0}".to_string())
        .call()
        .expect("field.set dispatches");
    assert_eq!(result, OperatorResult::Finished);
    app.update();

    assert_eq!(
        app.world().get::<Node>(node).map(|node| node.width),
        Some(Val::Px(120.0)),
        "the edit reached the entity it named"
    );
    assert_eq!(
        open_item(&app).stack_size,
        0,
        "and left the open definition alone"
    );
}

#[test]
fn a_definition_saves_back_to_the_file_it_was_opened_from() {
    let (mut app, tmp) = editor_with_items();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<ItemDef>>()
        .add(ItemDef::default());
    let flat = jackdaw::definition_assets::write_definition_file(
        app.world(),
        &item_type(),
        "sword",
        &handle.untyped(),
    )
    .expect("the file is written");
    let nested = tmp
        .path()
        .join("assets/content/items/weapons/sword.item.bsn");
    std::fs::create_dir_all(nested.parent().expect("a parent")).expect("the directory is made");
    std::fs::rename(&flat, &nested).expect("the file moves into its subdirectory");

    call(
        &mut app,
        "asset.open",
        &[("path", nested.to_string_lossy().into_owned().into())],
    );
    call(
        &mut app,
        "asset.set",
        &[("field", "stack_size".into()), ("value", "5".into())],
    );
    call(&mut app, "asset.save", &[]);

    let written = std::fs::read_to_string(&nested).expect("the file reads");
    assert!(written.contains("stack_size: 5"), "got:\n{written}");
    assert!(
        !flat.exists(),
        "the save does not leave a second file where the name alone would put it"
    );
}

#[test]
fn deleting_a_definition_closes_its_card_and_drops_the_entry() {
    let (mut app, tmp) = editor_with_items();
    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );
    let path = tmp.path().join("assets/content/items/torch.item.bsn");

    call(
        &mut app,
        "asset.delete",
        &[("path", path.to_string_lossy().into_owned().into())],
    );

    assert!(!path.exists(), "the file is gone");
    assert!(app.world().resource::<OpenDefinition>().0.is_none());
    assert!(
        app.world()
            .resource::<DefinitionRegistry>()
            .get("item", "torch")
            .is_none()
    );
}

#[test]
fn creating_a_definition_refuses_a_name_whose_file_is_already_there() {
    let (mut app, tmp) = editor_with_items();
    let path = tmp.path().join("assets/content/items/torch.item.bsn");
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory is made");
    std::fs::write(&path, "#torch ItemDef(stack_size: 7)\n").expect("the file is written");

    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );

    assert_eq!(
        std::fs::read_to_string(&path).expect("the file reads"),
        "#torch ItemDef(stack_size: 7)\n",
        "the file that was already there is left alone"
    );
}

#[test]
fn unregistering_a_definition_type_drops_its_entries_and_closes_the_card() {
    let (mut app, _tmp) = editor_with_items();
    call(
        &mut app,
        "asset.new",
        &[("type", "item".into()), ("name", "torch".into())],
    );
    assert!(app.world().resource::<OpenDefinition>().0.is_some());

    app.world_mut()
        .resource_mut::<DefinitionAssetTypes>()
        .unregister("item");
    app.update();

    assert!(
        app.world().resource::<OpenDefinition>().0.is_none(),
        "the card goes with the type that registered it"
    );
    assert!(
        app.world()
            .resource::<DefinitionRegistry>()
            .names_of("item")
            .is_empty()
    );
}

#[test]
fn a_material_file_is_not_deleted_through_the_definition_operator() {
    let (mut app, tmp) = editor_with_items();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    jackdaw::material_assets::write_material_file(app.world(), "slate", &handle)
        .expect("the material file is written");
    let path = tmp.path().join("assets/materials/slate.material.bsn");

    call(
        &mut app,
        "asset.delete",
        &[("path", path.to_string_lossy().into_owned().into())],
    );

    assert!(
        path.is_file(),
        "materials are removed by the tool that keeps their registry in step"
    );
}

#[test]
fn a_definition_file_reports_its_registered_kind() {
    let (app, _tmp) = editor_with_items();
    let types = app.world().resource::<DefinitionAssetTypes>();
    let matched = types
        .for_file(std::path::Path::new("assets/content/items/torch.item.bsn"))
        .expect("a registered type claims the file");
    assert_eq!(matched.kind, "item");
    assert!(
        types
            .for_file(std::path::Path::new("assets/scenes/level.bsn"))
            .is_none(),
        "a plain scene is still a scene"
    );
}

/// Materials are loaded with their textures by the material browser, and the
/// definition registry lists them alongside every other kind.
#[test]
fn the_material_directory_still_loads_and_lists_as_a_definition() {
    let (mut app, tmp) = editor_with_items();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            perceptual_roughness: 0.31,
            ..default()
        });
    jackdaw::material_assets::write_material_file(app.world(), "slate", &handle)
        .expect("the material file is written");
    assert!(
        tmp.path()
            .join("assets/materials/slate.material.bsn")
            .is_file()
    );

    let scan = jackdaw::material_assets::rescan_material_files(app.world_mut());
    assert_eq!(scan.added, vec!["slate".to_string()]);
    app.world_mut()
        .resource_mut::<jackdaw::material_assets::MaterialRegistry>()
        .add_saved("slate".to_string(), handle);
    app.update();

    let entry_path = app
        .world()
        .resource::<DefinitionRegistry>()
        .get(MATERIAL_KIND, "slate")
        .expect("the material lists as a definition")
        .path
        .clone();
    assert_eq!(
        entry_path,
        tmp.path().join("assets/materials/slate.material.bsn")
    );
}

#[test]
fn a_material_saved_by_its_own_operator_carries_what_asset_set_wrote() {
    let (mut app, tmp) = editor_with_items();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    jackdaw::material_assets::write_material_file(app.world(), "slate", &handle)
        .expect("the material file is written");
    app.world_mut()
        .resource_mut::<jackdaw::material_assets::MaterialRegistry>()
        .add_saved("slate".to_string(), handle.clone());
    app.world_mut()
        .insert_resource(jackdaw::material_preview::MaterialPreviewState {
            active_material: Some(handle),
            ..default()
        });
    app.update();

    call(
        &mut app,
        "asset.open",
        &[("path", "assets/materials/slate.material.bsn".into())],
    );
    call(
        &mut app,
        "asset.set",
        &[("field", "metallic".into()), ("value", "0.75".into())],
    );
    call(&mut app, "material.save", &[("material", "slate".into())]);
    app.update();

    let written = std::fs::read_to_string(tmp.path().join("assets/materials/slate.material.bsn"))
        .expect("the file reads");
    assert!(written.contains("metallic: 0.75"), "got:\n{written}");
}

/// A material's fields are filled in through the definition operators, with
/// no panel in the way.
#[test]
fn a_material_is_opened_and_its_fields_set_through_the_definition_operators() {
    let (mut app, tmp) = editor_with_items();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    jackdaw::material_assets::write_material_file(app.world(), "slate", &handle)
        .expect("the material file is written");
    app.world_mut()
        .resource_mut::<jackdaw::material_assets::MaterialRegistry>()
        .add_saved("slate".to_string(), handle.clone());
    app.update();

    call(
        &mut app,
        "asset.open",
        &[("path", "assets/materials/slate.material.bsn".into())],
    );
    call(
        &mut app,
        "asset.set",
        &[
            ("field", "perceptual_roughness".into()),
            ("value", "0.25".into()),
        ],
    );
    call(&mut app, "asset.save", &[]);

    let roughness = app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&handle)
        .expect("the material the scene is using")
        .perceptual_roughness;
    assert!(
        (roughness - 0.25).abs() < f32::EPSILON,
        "the edit lands on the loaded material, not on a copy"
    );
    let written = std::fs::read_to_string(tmp.path().join("assets/materials/slate.material.bsn"))
        .expect("the file reads");
    assert!(
        written.contains("perceptual_roughness: 0.25"),
        "got:\n{written}"
    );
}
