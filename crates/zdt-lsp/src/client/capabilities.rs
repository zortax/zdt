use lsp_types::{
    ClientCapabilities, CompletionClientCapabilities, CompletionItemCapability, InsertTextMode,
    MarkupKind, TextDocumentClientCapabilities,
};

/// What this editor tells a server it can do.
///
/// Deliberately modest. A capability claimed and not honoured is worse than one not claimed: the
/// server changes what it sends, and the difference shows up as something quietly not working.
pub(super) fn capabilities() -> ClientCapabilities {
    ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            completion: Some(CompletionClientCapabilities {
                completion_item: Some(CompletionItemCapability {
                    snippet_support: Some(false),
                    documentation_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    insert_text_mode_support: Some(lsp_types::InsertTextModeSupport {
                        value_set: vec![InsertTextMode::AS_IS],
                    }),
                    // The popup asks for this once the caret rests on a row. Asking for every
                    // row up front means a hundred paragraphs nobody will read.
                    resolve_support: Some(lsp_types::CompletionItemCapabilityResolveSupport {
                        properties: vec![
                            "documentation".to_owned(),
                            "detail".to_owned(),
                            "additionalTextEdits".to_owned(),
                        ],
                    }),
                    // The line beside the label. Claimed because the popup draws it; a server
                    // that sends it unasked is one whose extra field would be ignored.
                    label_details_support: Some(true),
                    ..CompletionItemCapability::default()
                }),
                ..CompletionClientCapabilities::default()
            }),
            hover: Some(lsp_types::HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                ..lsp_types::HoverClientCapabilities::default()
            }),
            signature_help: Some(lsp_types::SignatureHelpClientCapabilities {
                signature_information: Some(lsp_types::SignatureInformationSettings {
                    documentation_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    // Which parameter is being typed, which is the whole point of the panel.
                    parameter_information: Some(lsp_types::ParameterInformationSettings {
                        label_offset_support: Some(true),
                    }),
                    active_parameter_support: Some(true),
                }),
                context_support: Some(false),
                ..lsp_types::SignatureHelpClientCapabilities::default()
            }),
            // Claimed with `prepare_support`, because the rename box asks first: a key pressed on
            // a keyword should say "no" before somebody has typed a new name for it.
            rename: Some(lsp_types::RenameClientCapabilities {
                prepare_support: Some(true),
                ..lsp_types::RenameClientCapabilities::default()
            }),
            // The literal form, because a bare `Command` cannot say what kind of action it is and
            // the picker groups by kind. Resolution is claimed because servers compute an action's
            // edit only for the one that is chosen.
            code_action: Some(lsp_types::CodeActionClientCapabilities {
                code_action_literal_support: Some(lsp_types::CodeActionLiteralSupport {
                    code_action_kind: lsp_types::CodeActionKindLiteralSupport {
                        value_set: vec![
                            String::new(),
                            "quickfix".to_owned(),
                            "refactor".to_owned(),
                            "refactor.extract".to_owned(),
                            "refactor.inline".to_owned(),
                            "refactor.rewrite".to_owned(),
                            "source".to_owned(),
                            "source.organizeImports".to_owned(),
                        ],
                    },
                }),
                resolve_support: Some(lsp_types::CodeActionCapabilityResolveSupport {
                    properties: vec!["edit".to_owned()],
                }),
                is_preferred_support: Some(true),
                data_support: Some(true),
                ..lsp_types::CodeActionClientCapabilities::default()
            }),
            document_symbol: Some(lsp_types::DocumentSymbolClientCapabilities {
                hierarchical_document_symbol_support: Some(true),
                ..lsp_types::DocumentSymbolClientCapabilities::default()
            }),
            document_highlight: Some(lsp_types::DocumentHighlightClientCapabilities::default()),
            declaration: Some(lsp_types::GotoCapability::default()),
            type_definition: Some(lsp_types::GotoCapability::default()),
            implementation: Some(lsp_types::GotoCapability::default()),
            formatting: Some(lsp_types::DocumentFormattingClientCapabilities::default()),
            range_formatting: Some(lsp_types::DocumentRangeFormattingClientCapabilities::default()),
            publish_diagnostics: Some(lsp_types::PublishDiagnosticsClientCapabilities {
                version_support: Some(true),
                ..lsp_types::PublishDiagnosticsClientCapabilities::default()
            }),
            synchronization: Some(lsp_types::TextDocumentSyncClientCapabilities {
                did_save: Some(true),
                ..lsp_types::TextDocumentSyncClientCapabilities::default()
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        workspace: Some(lsp_types::WorkspaceClientCapabilities {
            // A rename crosses files, so the edit that carries it does too. `document_changes`
            // makes the edit versioned, so an edit against a buffer that has moved on is refused.
            // Unversioned, it would be applied to the wrong text.
            workspace_edit: Some(lsp_types::WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                resource_operations: Some(vec![
                    lsp_types::ResourceOperationKind::Create,
                    lsp_types::ResourceOperationKind::Rename,
                    lsp_types::ResourceOperationKind::Delete,
                ]),
                failure_handling: Some(lsp_types::FailureHandlingKind::Abort),
                ..lsp_types::WorkspaceEditClientCapabilities::default()
            }),
            symbol: Some(lsp_types::WorkspaceSymbolClientCapabilities::default()),
            execute_command: Some(lsp_types::DynamicRegistrationClientCapabilities::default()),
            ..lsp_types::WorkspaceClientCapabilities::default()
        }),
        window: Some(lsp_types::WindowClientCapabilities {
            work_done_progress: Some(true),
            ..lsp_types::WindowClientCapabilities::default()
        }),
        general: Some(lsp_types::GeneralClientCapabilities {
            position_encodings: Some(vec![
                lsp_types::PositionEncodingKind::UTF8,
                lsp_types::PositionEncodingKind::UTF16,
            ]),
            ..lsp_types::GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    }
}

/// Whether a server wants changes one at a time or the whole file each time.
#[must_use]
pub fn wants_incremental(capabilities: &lsp_types::ServerCapabilities) -> bool {
    use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

    match &capabilities.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(kind)) => *kind == TextDocumentSyncKind::INCREMENTAL,
        Some(TextDocumentSyncCapability::Options(options)) => {
            options.change == Some(TextDocumentSyncKind::INCREMENTAL)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{capabilities, wants_incremental};

    #[test]
    fn incremental_sync_is_only_claimed_when_it_is_asked_for() {
        use lsp_types::{
            ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
            TextDocumentSyncOptions,
        };

        let full = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            ..ServerCapabilities::default()
        };
        assert!(!wants_incremental(&full));

        let incremental = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::INCREMENTAL,
            )),
            ..ServerCapabilities::default()
        };
        assert!(wants_incremental(&incremental));

        let spelled_out = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    ..TextDocumentSyncOptions::default()
                },
            )),
            ..ServerCapabilities::default()
        };
        assert!(wants_incremental(&spelled_out));

        // Said nothing: whole text, which every server understands.
        assert!(!wants_incremental(&ServerCapabilities::default()));
    }

    #[test]
    fn what_is_claimed_is_what_is_honoured() {
        let claimed = capabilities();
        let completion = claimed
            .text_document
            .as_ref()
            .and_then(|text| text.completion.as_ref())
            .and_then(|completion| completion.completion_item.as_ref())
            .expect("completion is claimed");

        assert_eq!(
            completion.snippet_support,
            Some(false),
            "snippets are not expanded yet, so they are not claimed"
        );
        assert!(
            completion.resolve_support.is_some(),
            "the popup asks for documentation a row at a time, so resolution is claimed"
        );
    }

    #[test]
    fn every_request_that_is_written_is_also_claimed() {
        // A capability claimed and not honoured is worse than one not claimed. The server
        // changes what it sends, and the difference shows up as something quietly not working.
        // The reverse costs too: a strict server may refuse a request whose capability is absent.
        let claimed = capabilities();
        let text = claimed
            .text_document
            .as_ref()
            .expect("text-document capabilities are claimed");

        assert!(text.declaration.is_some(), "declaration is asked for");
        assert!(
            text.type_definition.is_some(),
            "type_definition is asked for"
        );
        assert!(text.implementation.is_some(), "implementation is asked for");
        assert!(text.signature_help.is_some(), "signature_help is asked for");
        assert!(text.rename.is_some(), "rename is asked for");
        assert!(text.code_action.is_some(), "code_action is asked for");
        assert!(
            text.document_symbol.is_some(),
            "document_symbol is asked for"
        );
        assert!(
            text.document_highlight.is_some(),
            "document_highlight is asked for"
        );
        assert!(
            text.range_formatting.is_some(),
            "range_formatting is asked for"
        );

        let workspace = claimed
            .workspace
            .as_ref()
            .expect("workspace capabilities are claimed");
        assert!(
            workspace.symbol.is_some(),
            "workspace symbols are asked for"
        );
        assert!(
            workspace.execute_command.is_some(),
            "a code action's command is run"
        );
    }

    #[test]
    fn a_rename_says_it_will_ask_first() {
        // That is what lets a key pressed on a keyword say "no" before somebody types a new
        // name for it.
        let rename = capabilities()
            .text_document
            .and_then(|text| text.rename)
            .expect("rename is claimed");
        assert_eq!(rename.prepare_support, Some(true));
    }
}
