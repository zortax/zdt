//! Who last changed this line, and when.

use crate::workspace::Workspace;

/// Who last touched a line, in the status line.
pub(super) fn blame(workspace: &Workspace, path: &std::path::Path, line: usize) {
    let (path, workspace) = (path.to_path_buf(), workspace.clone());
    zdt_view::detached(async move {
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
/// Blocking. It answers nothing when the file is untracked or git is absent. Both mean "there is
/// nothing to say".
pub(super) fn blame_line(path: &std::path::Path, line: usize) -> Option<String> {
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
        Some(when) => Some(format!("{author}, {} — {summary}", zdt_gitui::ago(when))),
        None => Some(format!("{author} — {summary}")),
    }
}
