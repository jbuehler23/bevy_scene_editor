//! The header line an asset `.bsn` file carries, naming the reflect type it
//! holds.
//!
//! The header sits in the leading comment block, on the line after the
//! version stamp when one is present. It is a hint for anything reading the
//! file before it parses; the first document root's type is the truth.

use bevy::ecs::entity::Entity;

use crate::SceneBsnAst;
use crate::catalog::asset_value_from_root;

/// The comment marker that introduces an asset file's type header.
pub const ASSET_HEADER: &str = "// jackdaw asset ";

/// Prepend the header naming the type an asset file holds.
pub fn with_asset_header(type_path: &str, body: &str) -> String {
    format!("{ASSET_HEADER}{type_path}\n{body}")
}

/// The type an asset file's header names, read from its leading comment lines.
pub fn read_asset_header(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(type_path) = line.strip_prefix(ASSET_HEADER) {
            let type_path = type_path.trim();
            return (!type_path.is_empty()).then(|| type_path.to_string());
        }
        if !line.starts_with("//") {
            return None;
        }
    }
    None
}

/// The type path a document root names, or `None` when the root names none.
pub fn root_type_path(ast: &SceneBsnAst, root: Entity) -> Option<String> {
    asset_value_from_root(ast, root).map(|(type_path, _)| type_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_bsn_text;

    const STAMP: &str = "// jackdaw 0.19.0 | bevy 0.19";

    #[test]
    fn an_asset_file_s_header_round_trips() {
        let text = with_asset_header("my_game::content::ItemDef", "my_game::content::ItemDef {}");
        assert_eq!(
            read_asset_header(&text).as_deref(),
            Some("my_game::content::ItemDef")
        );
    }

    #[test]
    fn a_header_below_the_version_stamp_is_found() {
        let text = format!(
            "{STAMP}\n{}",
            with_asset_header("my_game::content::ItemDef", "my_game::content::ItemDef {}")
        );
        assert_eq!(
            read_asset_header(&text).as_deref(),
            Some("my_game::content::ItemDef")
        );
    }

    #[test]
    fn a_file_without_a_header_names_no_type() {
        assert!(read_asset_header("my_game::content::ItemDef {}").is_none());
        assert!(read_asset_header(&format!("{STAMP}\nmy_game::content::ItemDef {{}}")).is_none());
    }

    #[test]
    fn a_comment_below_the_body_is_not_a_header() {
        let text =
            format!("my_game::content::ItemDef {{}}\n{ASSET_HEADER}my_game::content::Impostor\n");
        assert!(read_asset_header(&text).is_none());
    }

    #[test]
    fn a_root_reports_the_type_it_names() {
        let ast = parse_bsn_text("#Sword\nmy_game::content::ItemDef { damage: 3.0 }")
            .expect("document parses");
        let root = *ast.roots.first().expect("one root");
        assert_eq!(
            root_type_path(&ast, root).as_deref(),
            Some("my_game::content::ItemDef")
        );
    }
}
