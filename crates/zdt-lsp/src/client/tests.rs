use lsp_types::{GotoDefinitionResponse, Url};

use super::capabilities::capabilities;
use super::symbol::{locations, symbols};

#[test]
fn the_three_shapes_of_a_definition_answer_are_one_list() {
    let uri = Url::parse("file:///a.rs").expect("a uri");
    let range = lsp_types::Range::default();
    let one = lsp_types::Location {
        uri: uri.clone(),
        range,
    };

    assert_eq!(
        locations(Some(GotoDefinitionResponse::Scalar(one.clone()))).len(),
        1
    );
    assert_eq!(
        locations(Some(GotoDefinitionResponse::Array(vec![
            one.clone(),
            one.clone()
        ])))
        .len(),
        2
    );
    assert_eq!(
        locations(Some(GotoDefinitionResponse::Link(vec![
            lsp_types::LocationLink {
                origin_selection_range: None,
                target_uri: uri,
                target_range: range,
                target_selection_range: range,
            }
        ])))
        .len(),
        1
    );
    assert!(locations(None).is_empty());
}

#[test]
fn a_workspace_edit_is_versioned_and_may_move_files() {
    // Unversioned, an edit that arrives after the buffer has moved on goes into the wrong text
    // with no way to tell. Without the resource operations, a rename that moves a file loses the
    // move. Renaming a Rust module does exactly that.
    let edit = capabilities()
        .workspace
        .and_then(|workspace| workspace.workspace_edit)
        .expect("workspace edits are claimed");
    assert_eq!(edit.document_changes, Some(true));
    assert_eq!(
        edit.resource_operations.map(|ops| ops.len()),
        Some(3),
        "create, rename and delete"
    );
}

#[test]
fn both_shapes_of_workspace_symbol_flatten_to_one() {
    use lsp_types::{
        Location, OneOf, Position, Range, SymbolInformation, SymbolKind, WorkspaceSymbol,
        WorkspaceSymbolResponse,
    };

    let uri = Url::parse("file:///project/src/lib.rs").expect("a url");
    let range = Range::new(Position::new(3, 0), Position::new(3, 8));

    #[allow(deprecated)]
    let flat = WorkspaceSymbolResponse::Flat(vec![SymbolInformation {
        name: "run".to_owned(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        location: Location {
            uri: uri.clone(),
            range,
        },
        container_name: Some("app".to_owned()),
    }]);
    let found = symbols(Some(flat));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "run");
    assert_eq!(found[0].range, range);
    assert_eq!(found[0].container.as_deref(), Some("app"));

    // The same symbol said the other way round comes out identical, which is the whole point:
    // a picker over these must not be able to tell which shape the server chose.
    let nested = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
        name: "run".to_owned(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        container_name: Some("app".to_owned()),
        location: OneOf::Left(Location {
            uri: uri.clone(),
            range,
        }),
        data: None,
    }]);
    assert_eq!(symbols(Some(nested)), found);
}

#[test]
fn a_symbol_with_no_range_lands_at_the_top_of_its_file() {
    // Wrong by a screenful and right by a file, and a file is what somebody picking a symbol
    // out of a project of ten thousand was actually after.
    use lsp_types::{
        OneOf, SymbolKind, WorkspaceLocation, WorkspaceSymbol, WorkspaceSymbolResponse,
    };

    let uri = Url::parse("file:///project/src/lib.rs").expect("a url");
    let answer = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
        name: "run".to_owned(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        container_name: None,
        location: OneOf::Right(WorkspaceLocation { uri: uri.clone() }),
        data: None,
    }]);

    let found = symbols(Some(answer));
    assert_eq!(found[0].uri, uri);
    assert_eq!(found[0].range, lsp_types::Range::default());
}

#[test]
fn nothing_at_all_is_an_empty_list_rather_than_an_error() {
    assert!(symbols(None).is_empty());
    assert!(locations(None).is_empty());
}
