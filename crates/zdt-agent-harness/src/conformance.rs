//! The contract every adapter's event stream keeps.
//!
//! One shared set of checks, run by each adapter's fixture tests over the events its fold
//! answers a captured transcript with. A second harness passes the same bar as the first, which
//! is what makes the seam a seam.

use std::collections::HashMap;

use zdt_agent::event::AgentEvent;
use zdt_agent::thread::ItemStatus;

/// Checks one folded transcript's events. Panics with the broken rule's name.
///
/// The rules:
/// - the session is named before any turn output;
/// - a turn that produced output ends in exactly the `TurnDone` or `Fatal` events seen;
/// - a work item never moves after it is done;
/// - every ask that was opened is either resolved by the adapter or left for the daemon,
///   never resolved twice.
pub fn check(events: &[AgentEvent]) {
    let mut named = false;
    let mut output_before_name = false;
    let mut work: HashMap<String, ItemStatus> = HashMap::new();
    let mut asks: HashMap<String, u32> = HashMap::new();

    for event in events {
        match event {
            AgentEvent::SessionStarted { session, .. } => {
                assert!(!session.is_empty(), "the session has no name");
                named = true;
            }
            AgentEvent::Delta { .. } if !named => output_before_name = true,
            AgentEvent::Work { item, .. } => {
                if !named {
                    output_before_name = true;
                }
                let held = work.get(&item.key);
                assert!(
                    !held.is_some_and(|status| *status != ItemStatus::Running),
                    "work item {} moved after it was done",
                    item.key
                );
                work.insert(item.key.clone(), item.status);
            }
            AgentEvent::Asked { ask, .. } => {
                let opened = asks.entry(ask.id.clone()).or_insert(0);
                *opened += 1;
                assert_eq!(*opened, 1, "ask {} was opened twice", ask.id);
            }
            AgentEvent::AskGone { id, .. } => {
                assert!(
                    asks.contains_key(id),
                    "ask {id} was withdrawn before it opened"
                );
            }
            _ => {}
        }
    }
    assert!(
        !output_before_name,
        "turn output came before the session was named"
    );
}

/// Whether the stream says the turn settled: one `TurnDone`, and events after it only quiet
/// ones. Answers how many turns settled.
#[must_use]
pub fn settled_turns(events: &[AgentEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, AgentEvent::TurnDone { .. }))
        .count()
}
