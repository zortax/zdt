//! The captured transcripts, folded and checked.
//!
//! `simple.ndjson` is a real turn against codex-cli 0.146.0: a file written, a command run, two
//! messages streamed. `declined.ndjson` is a turn under a read-only sandbox whose file-change
//! approval was declined. Recapture with the driver script noted in the repository's agent plan
//! when the CLI moves.

use std::sync::{Arc, Mutex};

use zdt_agent::event::{AgentEvent, StreamKind};
use zdt_agent::thread::{ItemStatus, ThreadId, ToolKind};
use zdt_agent_codex::fold::{Folder, State};

/// Folds one fixture, answering the events and the frames written back.
fn folded(fixture: &str) -> (Vec<AgentEvent>, Vec<serde_json::Value>) {
    let state = Arc::new(Mutex::new(State {
        turn_open: true,
        ..State::default()
    }));
    let mut folder = Folder::new(ThreadId(7), state);
    let mut events = Vec::new();
    let mut writes = Vec::new();
    for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).expect("the fixture is JSON");
        let fold = folder.take(&value);
        events.extend(fold.events);
        writes.extend(fold.writes);
    }
    (events, writes)
}

#[test]
fn a_real_turn_folds_into_a_conforming_stream() {
    let (events, _) = folded(include_str!("fixtures/simple.ndjson"));
    zdt_agent_harness::conformance::check(&events);
    assert_eq!(zdt_agent_harness::conformance::settled_turns(&events), 1);
}

#[test]
fn the_session_is_named_by_the_thread_answer() {
    let (events, _) = folded(include_str!("fixtures/simple.ndjson"));
    let named = events.iter().find_map(|event| match event {
        AgentEvent::SessionStarted { session, model, .. } => Some((session.clone(), model.clone())),
        _ => None,
    });
    let (session, model) = named.expect("the session is named");
    assert_eq!(session, "01a02f3b-fd88-78d2-abed-308bb7c1cef5");
    assert!(!model.is_empty());
}

#[test]
fn streamed_prose_arrives_as_assistant_deltas() {
    let (events, _) = folded(include_str!("fixtures/simple.ndjson"));
    let streamed: String = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Delta {
                kind: StreamKind::Assistant,
                text,
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(streamed.contains("create the file"), "got: {streamed}");
    // Nothing is said twice: the completed message repeats what streamed, and is dropped.
    assert_eq!(streamed.matches("Created `hello.txt`").count(), 1);
}

#[test]
fn the_file_change_and_the_command_become_work_rows() {
    let (events, _) = folded(include_str!("fixtures/simple.ndjson"));
    let work: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Work { item, .. } => Some(item.clone()),
            _ => None,
        })
        .collect();
    let edit = work
        .iter()
        .find(|item| item.tool == ToolKind::Edit)
        .expect("the file change is a row");
    assert_eq!(edit.summary, "hello.txt");
    assert!(work.iter().any(|item| {
        item.tool == ToolKind::Execute
            && item.status == ItemStatus::Ok
            && item.summary.contains("od -An")
    }));
}

#[test]
fn usage_carries_the_window() {
    let (events, _) = folded(include_str!("fixtures/simple.ndjson"));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::Usage {
            context_tokens,
            context_limit,
            ..
        } if *context_tokens > 0 && *context_limit > 0
    )));
}

#[test]
fn a_declined_approval_opens_an_ask_and_the_row_ends_declined() {
    let (events, _) = folded(include_str!("fixtures/declined.ndjson"));
    zdt_agent_harness::conformance::check(&events);
    let asked = events.iter().find_map(|event| match event {
        AgentEvent::Asked { ask, .. } => Some(ask.clone()),
        _ => None,
    });
    let ask = asked.expect("the approval is an ask");
    assert!(matches!(
        ask.kind,
        zdt_agent::ask::AskKind::Tool {
            tool: ToolKind::Edit,
            ..
        }
    ));
    // The person's decline came back over the wire; the fixture holds the resolution.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::AskGone { id, .. } if *id == ask.id))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::Work { item, .. } if item.status == ItemStatus::Declined
    )));
}

#[test]
fn a_stream_that_ends_mid_turn_is_fatal() {
    let state = Arc::new(Mutex::new(State {
        turn_open: true,
        ..State::default()
    }));
    let mut folder = Folder::new(ThreadId(7), Arc::clone(&state));
    let started: serde_json::Value = serde_json::json!({
        "method": "turn/started",
        "params": { "threadId": "t", "turn": { "id": "turn-1" } },
    });
    let _ = folder.take(&started);
    let ended = folder.ended();
    assert!(
        ended
            .iter()
            .any(|event| matches!(event, AgentEvent::Fatal { .. }))
    );
}
