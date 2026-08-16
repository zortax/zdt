use lsp_types::{GotoDefinitionResponse, Url};

/// One thing a project declares, whichever shape the server said it in.
///
/// The protocol has two answers for "what is in this project": a flat list with locations, and a
/// nested one whose locations may be a file with no range in it. A picker wants one. So both are
/// flattened here, and the four callers stay ignorant of the difference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// What it is called.
    pub name: String,
    /// What kind of thing it is.
    pub kind: lsp_types::SymbolKind,
    /// What it is inside, when the server says.
    pub container: Option<String>,
    /// Which file it is in.
    pub uri: Url,
    /// Where in the file. The start of it when the server gave only a file.
    pub range: lsp_types::Range,
}

/// The two shapes a workspace-symbol answer comes in, as one list.
pub(super) fn symbols(answer: Option<lsp_types::WorkspaceSymbolResponse>) -> Vec<Symbol> {
    use lsp_types::{OneOf, WorkspaceSymbolResponse};

    match answer {
        None => Vec::new(),
        Some(WorkspaceSymbolResponse::Flat(found)) => found
            .into_iter()
            .map(|one| Symbol {
                name: one.name,
                kind: one.kind,
                container: one.container_name,
                uri: one.location.uri,
                range: one.location.range,
            })
            .collect(),
        Some(WorkspaceSymbolResponse::Nested(found)) => found
            .into_iter()
            .map(|one| {
                let (uri, range) = match one.location {
                    OneOf::Left(location) => (location.uri, location.range),
                    // A server may answer with a file and no range when the client says it will
                    // resolve. This client does not resolve, so the symbol goes to the top of the
                    // file. That is wrong by a screenful and right by a file, and a file is what
                    // somebody picking a symbol wanted.
                    OneOf::Right(partial) => (partial.uri, lsp_types::Range::default()),
                };
                Symbol {
                    name: one.name,
                    kind: one.kind,
                    container: one.container_name,
                    uri,
                    range,
                }
            })
            .collect(),
    }
}

/// The three shapes a "go to definition" answer comes in, as one list.
pub(super) fn locations(answer: Option<GotoDefinitionResponse>) -> Vec<lsp_types::Location> {
    match answer {
        Some(GotoDefinitionResponse::Scalar(one)) => vec![one],
        Some(GotoDefinitionResponse::Array(many)) => many,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| lsp_types::Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
        None => Vec::new(),
    }
}
