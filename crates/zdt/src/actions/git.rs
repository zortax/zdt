//! What the git keys do.
//!
//! Navigation and staging. What is *not* here is anything that rewrites history or touches a
//! branch: this editor shows what git says and stages a hunk, and everything past that is what
//! `<Leader>gg` opens lazygit for.

use zgui_editor::EditorHandle;

use crate::git::Git;
use crate::workspace::Workspace;

/// Carries out one `git.*` action.
pub fn run(workspace: &Workspace, leaf: &str, handle: Option<&EditorHandle>) {
    let Some(git) = zgui::reactive::use_local_context::<Git>() else {
        return;
    };
    let Some(handle) = handle else {
        return;
    };
    let Some(path) = workspace.current_buffer().and_then(|buffer| buffer.path) else {
        workspace.say("this buffer is not a file");
        return;
    };

    let line = handle.query(|snapshot| {
        let caret = snapshot.selections().primary().head;
        snapshot.rope().byte_to_line(caret)
    });
    let hunks = git.hunks(&path);

    match leaf {
        "next_hunk" | "previous_hunk" => {
            let found = if leaf == "next_hunk" {
                zdt_core::git::after(&hunks, line)
            } else {
                zdt_core::git::before(&hunks, line)
            };
            let Some(found) = found else {
                workspace.say("no changes");
                return;
            };
            let at = handle.query(|snapshot| {
                let rope = snapshot.rope();
                let line = found.line.min(rope.len_lines().saturating_sub(1));
                rope.char_to_byte(rope.line_to_char(line))
            });
            handle.command(zgui_editor::Command::SetSelections {
                selections: vec![zgui_editor::Selection::caret(at)],
                primary: 0,
            });
            handle.command(zgui_editor::Command::Scroll(
                zgui_editor::ScrollCmd::CursorCenter,
            ));
        }
        "preview_hunk" => match hunks.iter().find(|hunk| hunk.covers(line)) {
            Some(hunk) => workspace.say(format!(
                "{:?} at line {}, {} line{}",
                hunk.change,
                hunk.line + 1,
                hunk.count.max(1),
                if hunk.count == 1 { "" } else { "s" }
            )),
            None => workspace.say("nothing changed here"),
        },
        "stage_hunk" | "reset_hunk" => {
            // Staging one hunk means writing a patch to git's index, which is more than a gutter
            // needs to know how to do. Saying so beats a key that quietly stages the whole file.
            workspace.say(format!(
                "{} is not built yet; <Leader>gg opens lazygit",
                leaf.replace('_', " ")
            ));
        }
        "blame_line" => blame(workspace, &path, line),
        other => workspace.say(format!("git.{other} is not built yet")),
    }
}

/// Who last touched a line, in the status line.
fn blame(workspace: &Workspace, path: &std::path::Path, line: usize) {
    let (path, workspace) = (path.to_path_buf(), workspace.clone());
    crate::task::detached(async move {
        let said = {
            let path = path.clone();
            zgui::task::blocking(move || blame_line(&path, line)).await
        };
        match said {
            Some(said) => workspace.say(said),
            None => workspace.say("no blame for this line"),
        }
    });
}

/// What `git blame` says about one line, as one line.
///
/// Blocking. Nothing when the file is not tracked or git is not installed, which are both "there
/// is nothing to say" rather than errors.
fn blame_line(path: &std::path::Path, line: usize) -> Option<String> {
    let directory = path.parent()?;
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["blame", "--porcelain", "-L"])
        .arg(format!("{},{}", line + 1, line + 1))
        .arg("--")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut author = None;
    let mut when = None;
    let mut summary = None;
    for said in text.lines() {
        if let Some(rest) = said.strip_prefix("author ") {
            author = Some(rest.to_owned());
        } else if let Some(rest) = said.strip_prefix("author-time ") {
            when = rest.parse::<i64>().ok();
        } else if let Some(rest) = said.strip_prefix("summary ") {
            summary = Some(rest.to_owned());
        }
    }

    let author = author?;
    let summary = summary.unwrap_or_default();
    match when {
        Some(when) => Some(format!("{author}, {} — {summary}", ago(when))),
        None => Some(format!("{author} — {summary}")),
    }
}

/// How long ago a unix timestamp was, roughly.
///
/// Roughly on purpose: "3 months ago" is what anybody reads off a blame line, and the exact day is
/// what `git log` is for.
fn ago(when: i64) -> String {
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return "some time ago".to_owned();
    };
    let seconds = (now.as_secs() as i64 - when).max(0);

    let (count, unit) = match seconds {
        ..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_592_000 => (seconds / 86_400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_ago_reads_in_the_largest_unit_that_fits() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64;

        assert_eq!(ago(now), "just now");
        assert_eq!(ago(now - 120), "2 minutes ago");
        assert_eq!(ago(now - 3_600), "1 hour ago");
        assert_eq!(ago(now - 86_400 * 3), "3 days ago");
        assert_eq!(ago(now - 2_592_000 * 4), "4 months ago");
        assert_eq!(ago(now - 31_536_000 * 2), "2 years ago");
    }

    #[test]
    fn a_timestamp_in_the_future_is_not_a_negative_age() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64;
        assert_eq!(ago(now + 10_000), "just now");
    }
}
