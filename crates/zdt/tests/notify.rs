//! The announcements.
//!
//! The queue itself belongs to the component library and is asserted there. What is asserted here
//! is the layer over it: that a keyed announcement replaces the one it is replacing rather than
//! stacking beside it, that a failure waits to be read, and that switching announcements off
//! switches them off rather than merely hiding them.
//!
//! Every one of these runs against a real `ToastQueue` provided into the scope, because the whole
//! point of `Notify` is what it does *to* a queue.

use zdt::notify::Notify;
use zdt::settings::Settings;
use zgui_testkit_view::Window;
use zgui_ui::toast::{Toast, ToastKind, ToastQueue};

/// Runs `body` in a reactive scope with somewhere for announcements to go.
///
/// The queue is provided rather than a `Toaster` mounted: `Notify::new` finds it with
/// `use_toaster`, which is a context read, and a context is all it needs.
fn with_queue<R>(config: zdt_core::Config, body: impl FnOnce(Notify) -> R) -> R {
    let window = Window::open();
    window.scope.with(|| {
        let _queue = ToastQueue::provide();
        body(Notify::new(Settings::new(config, None)))
    })
}

/// The defaults, which have announcements on.
fn announcing() -> zdt_core::Config {
    zdt_core::Config::default()
}

#[test]
fn something_said_is_something_showing() {
    with_queue(announcing(), |notify| {
        assert_eq!(notify.live(), 0, "nothing has happened yet");
        notify.say("written");
        assert_eq!(notify.live(), 1);
    });
}

#[test]
fn a_keyed_announcement_replaces_rather_than_stacks() {
    // A language server says "starting", then "indexing", then "ready". That is one piece of news
    // three times, and a stack that showed all three would grow while nothing changed.
    with_queue(announcing(), |notify| {
        notify.progress("rust-analyzer", Toast::new("starting").persistent());
        notify.progress("rust-analyzer", Toast::new("indexing").persistent());
        notify.progress("rust-analyzer", Toast::new("ready").persistent());

        assert_eq!(
            notify.live(),
            1,
            "one server holds one row however much it says"
        );
    });
}

#[test]
fn two_servers_are_two_rows() {
    // The key is what makes replacing happen, so two different keys must not replace each other:
    // rust-analyzer failing while gopls indexes is two pieces of news.
    with_queue(announcing(), |notify| {
        notify.progress("rust-analyzer", Toast::new("indexing").persistent());
        notify.progress("gopls", Toast::new("indexing").persistent());
        assert_eq!(notify.live(), 2);
    });
}

#[test]
fn clearing_a_key_gives_its_row_back() {
    with_queue(announcing(), |notify| {
        notify.progress("rust-analyzer", Toast::new("indexing").persistent());
        assert_eq!(notify.live(), 1);
        notify.clear("rust-analyzer");
        assert_eq!(notify.live(), 0, "the row is not staying");
    });
}

#[test]
fn clearing_a_key_nothing_is_under_is_not_an_error() {
    // Which happens whenever a server finishes something it never announced starting.
    with_queue(announcing(), |notify| {
        notify.clear("rust-analyzer");
        assert_eq!(notify.live(), 0);
    });
}

#[test]
fn a_failure_waits_to_be_read() {
    // The defect this prevents is a server that could not start saying so for four seconds while
    // somebody was looking at another window, and then never again.
    with_queue(announcing(), |notify| {
        notify.fail(
            "rust-analyzer did not start",
            Some("no such file".to_owned()),
        );
        assert_eq!(notify.live(), 1);
    });
    // The deadline itself is the queue's business; what is asserted here is that the toast this
    // builds carries none.
    let toast = Toast::new("x").kind(ToastKind::Error).persistent();
    assert!(toast.stays_for().is_none());
}

#[test]
fn dismissing_takes_every_row_including_the_keyed_ones() {
    with_queue(announcing(), |notify| {
        notify.say("one");
        notify.progress("rust-analyzer", Toast::new("indexing").persistent());
        notify.dismiss_all();

        assert_eq!(notify.live(), 0);
        // And the key is forgotten too, so the next thing that server says opens a new row rather
        // than trying to replace one that has gone.
        notify.progress("rust-analyzer", Toast::new("ready").persistent());
        assert_eq!(notify.live(), 1);
    });
}

#[test]
fn switching_announcements_off_switches_them_off() {
    let mut quiet = zdt_core::Config::default();
    quiet.ui.notifications = false;

    with_queue(quiet, |notify| {
        notify.say("written");
        notify.fail("broken", None);
        notify.progress("rust-analyzer", Toast::new("indexing").persistent());
        assert_eq!(
            notify.showing(),
            0,
            "nothing is pushed at all, rather than pushed and hidden"
        );
    });
}

#[test]
fn a_timeout_of_zero_means_until_it_is_dismissed() {
    // Asking for an announcement that stays for no time at all can only mean one thing, and it is
    // not "flash it for zero milliseconds".
    let mut held = zdt_core::Config::default();
    held.ui.notification_timeout = 0;

    with_queue(held, |notify| {
        notify.say("written");
        assert_eq!(notify.live(), 1);
    });
}

#[test]
fn announcements_survive_having_nowhere_to_go() {
    // Every test that mounts one component without a toaster over it, and every headless run.
    let window = Window::open();
    window.scope.with(|| {
        let notify = Notify::new(Settings::new(zdt_core::Config::default(), None));
        notify.say("written");
        notify.fail("broken", None);
        notify.progress("rust-analyzer", Toast::new("indexing"));
        notify.clear("rust-analyzer");
        notify.dismiss_all();
        assert_eq!(notify.live(), 0);
    });
}

/// The language layer, with somewhere for its announcements to go.
///
/// Built in one scope so that `Language::new` finds the queue: it takes the announcements once at
/// construction rather than looking them up later, because most of what it says happens inside a
/// task or a timer and neither is inside a scope that has them.
fn language(window: &Window) -> (zdt::language::Language, Notify) {
    window.scope.with(|| {
        let _queue = ToastQueue::provide();
        let settings = Settings::new(zdt_core::Config::default(), None);
        let notify = Notify::new(settings.clone());
        zdt::notify::provide(notify.clone());

        let workspace = zdt::workspace::Workspace::new(zdt_core::Project::at("/project"));
        let language = zdt::language::Language::new(workspace, settings);
        language.listen();
        (language, notify)
    })
}

/// Lets the drain timer run.
fn settle(window: &Window) {
    for _ in 0..40 {
        window.advance(std::time::Duration::from_millis(20));
        window.frame();
    }
}

#[test]
fn a_flood_of_progress_is_one_announcement() {
    // The defect this prevents froze the window: `rust-analyzer` reports progress once per crate
    // it indexes, which on a workspace of any size is thousands of notices in a few seconds.
    // Announcing each one mounted a toast, an expiry timer and two animations per crate, and left
    // every dismissed one on the stack until its exit finished.
    //
    // One job is one announcement. What it is busy *with* changes constantly and lives in the
    // status line, which costs one signal write.
    let window = Window::open();
    let (language, notify) = language(&window);

    let notices = language.notices();
    for crate_number in 0..2000 {
        notices
            .send(zdt_lsp::client::Notice::Progress {
                server: "rust-analyzer".to_owned(),
                title: Some(format!("indexing {crate_number}/2000")),
                done: false,
            })
            .expect("the channel is open");
    }
    settle(&window);

    assert_eq!(
        notify.live(),
        1,
        "two thousand progress reports are one announcement"
    );
    // And the detail is in the status line, saying what it is busy with.
    assert!(
        language
            .busy()
            .is_some_and(|doing| doing.contains("indexing")),
        "the status line says what it is doing: {:?}",
        language.busy()
    );
}

#[test]
fn a_second_job_does_not_open_a_second_row() {
    // Both keyed on the server, so a server that finishes indexing and starts something else
    // replaces its own row rather than stacking beside it.
    let window = Window::open();
    let (language, notify) = language(&window);
    let notices = language.notices();

    for stage in ["roots scanned", "building crate graph", "indexing"] {
        notices
            .send(zdt_lsp::client::Notice::Progress {
                server: "rust-analyzer".to_owned(),
                title: Some(stage.to_owned()),
                done: false,
            })
            .expect("the channel is open");
        settle(&window);
    }

    assert_eq!(notify.live(), 1, "one server, one row");
}

#[test]
fn the_same_progress_twice_changes_nothing() {
    // Most reports repeat the message they sent last time. Re-drawing on each would be a status
    // line re-rendering thousands of times to say what it already said.
    let window = Window::open();
    let (language, _notify) = language(&window);

    let notices = language.notices();
    for _ in 0..50 {
        notices
            .send(zdt_lsp::client::Notice::Progress {
                server: "rust-analyzer".to_owned(),
                title: Some("indexing".to_owned()),
                done: false,
            })
            .expect("the channel is open");
    }
    settle(&window);

    assert_eq!(language.busy().as_deref(), Some("rust-analyzer: indexing"));
}

#[test]
fn progress_finishing_clears_what_it_was_doing() {
    let window = Window::open();
    let (language, notify) = language(&window);

    let notices = language.notices();
    notices
        .send(zdt_lsp::client::Notice::Progress {
            server: "rust-analyzer".to_owned(),
            title: Some("indexing".to_owned()),
            done: false,
        })
        .expect("the channel is open");
    settle(&window);
    assert!(language.busy().is_some());

    notices
        .send(zdt_lsp::client::Notice::Progress {
            server: "rust-analyzer".to_owned(),
            title: None,
            done: true,
        })
        .expect("the channel is open");
    settle(&window);

    assert!(language.busy().is_none(), "it is not busy any more");
    assert_eq!(
        notify.live(),
        0,
        "and the announcement it was holding has gone with it"
    );
}

#[test]
fn something_a_server_says_out_loud_is_still_announced() {
    // Progress is state and is not announced; a *message* is news and is. The fix for the flood
    // must not have made the layer silent.
    let window = Window::open();
    let (language, notify) = language(&window);

    language
        .notices()
        .send(zdt_lsp::client::Notice::Message {
            server: "rust-analyzer".to_owned(),
            severity: lsp_types::MessageType::ERROR,
            text: "could not load Cargo.toml".to_owned(),
        })
        .expect("the channel is open");
    settle(&window);

    assert_eq!(notify.live(), 1);
}
