//! The commit history.
//!
//! Walked in-process, a page at a time. A history is however many commits somebody has made, and
//! the panel shows a screenful; walking all of them to draw twelve would be the same mistake as
//! building two thousand rows to show twelve.

use crate::repo::{Error, Repo};

/// One commit, as the panel draws it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Commit {
    /// Its full identifier.
    pub id: String,
    /// The first seven characters of it, which is what a person reads.
    pub short: String,
    /// The first line of the message.
    pub summary: String,
    /// Everything after that, when there is anything.
    pub body: String,
    /// Who wrote it.
    pub author: String,
    /// Their address.
    pub email: String,
    /// When, in seconds since the epoch.
    pub when: i64,
    /// What it came from. Two parents is a merge, none is the first commit.
    pub parents: Vec<String>,
}

impl Commit {
    /// Whether it joined two histories.
    #[must_use]
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// The `limit` commits reachable from `start`, newest first.
///
/// `start` is a revision as git would spell it: a branch name, a hash, or `HEAD`. `None` means
/// whatever is checked out.
///
/// # Errors
///
/// When the revision does not resolve, or the object store cannot be read. An empty repository
/// answers an empty list. A project on its first commit is an ordinary project, and the panel
/// should open on it.
pub fn log(repo: &Repo, start: Option<&str>, limit: usize) -> Result<Vec<Commit>, Error> {
    let git = repo.git();

    let head = match start {
        Some(revision) => match git.rev_parse_single(revision) {
            Ok(found) => found.detach(),
            Err(error) => return Err(Error::git(error)),
        },
        None => match git.head_id() {
            Ok(found) => found.detach(),
            // No commits yet.
            Err(_) => return Ok(Vec::new()),
        },
    };

    let mut out = Vec::with_capacity(limit.min(1024));
    let walk = git
        .rev_walk([head])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(Error::git)?;

    for step in walk {
        if out.len() >= limit {
            break;
        }
        let info = step.map_err(Error::git)?;
        let object = git.find_commit(info.id).map_err(Error::git)?;
        out.push(one(&object)?);
    }
    Ok(out)
}

/// One commit, read.
///
/// # Errors
///
/// When the object cannot be decoded.
pub fn find(repo: &Repo, revision: &str) -> Result<Commit, Error> {
    let id = repo
        .git()
        .rev_parse_single(revision)
        .map_err(Error::git)?
        .detach();
    let object = repo.git().find_commit(id).map_err(Error::git)?;
    one(&object)
}

/// One `gix` commit, as the panel wants it.
fn one(object: &gix::Commit<'_>) -> Result<Commit, Error> {
    let id = object.id().to_string();
    let short = id.chars().take(7).collect();

    let message = object.message().map_err(Error::git)?;
    let summary = message.summary().to_string();
    // Everything after the first line, with the blank line that separates them taken off. A body
    // that is only whitespace is no body at all.
    let body = message
        .body
        .map(|body| body.to_string().trim().to_owned())
        .filter(|body| !body.is_empty())
        .unwrap_or_default();

    let author = object.author().map_err(Error::git)?;

    Ok(Commit {
        id,
        short,
        summary,
        body,
        author: author.name.to_string(),
        email: author.email.to_string(),
        when: author.time().map(|time| time.seconds).unwrap_or_default(),
        parents: object.parent_ids().map(|id| id.to_string()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{find, log};
    use crate::repo::testing::Temp;

    #[test]
    fn a_history_comes_back_newest_first() {
        let temp = Temp::new("log-order");
        temp.commit("a.txt", "one\n", "first");
        temp.commit("b.txt", "two\n", "second");
        temp.commit("c.txt", "three\n", "third");

        let found = log(&temp.repo(), None, 10).expect("it walks");
        assert_eq!(
            found
                .iter()
                .map(|one| one.summary.as_str())
                .collect::<Vec<_>>(),
            ["third", "second", "first"]
        );
    }

    #[test]
    fn the_limit_is_a_limit() {
        // The whole reason the walk is lazy: a panel showing twelve rows must not read thirty
        // thousand commits to draw them.
        let temp = Temp::new("log-limit");
        for n in 0..5 {
            temp.commit("a.txt", &format!("{n}\n"), &format!("commit {n}"));
        }
        assert_eq!(log(&temp.repo(), None, 2).expect("it walks").len(), 2);
    }

    #[test]
    fn a_repository_with_no_commits_is_an_empty_list() {
        // A project on its first commit is a perfectly ordinary project, and the panel opens on it.
        let temp = Temp::new("log-empty");
        assert!(log(&temp.repo(), None, 10).expect("it walks").is_empty());
    }

    #[test]
    fn a_commit_carries_everything_the_panel_draws() {
        let temp = Temp::new("log-fields");
        temp.write("a.txt", "one\n");
        temp.run(&["add", "a.txt"]);
        temp.run(&["commit", "-m", "the summary\n\nthe body, which is longer."]);

        let found = find(&temp.repo(), "HEAD").expect("it reads");
        assert_eq!(found.summary, "the summary");
        assert_eq!(found.body, "the body, which is longer.");
        assert_eq!(found.author, "Test");
        assert_eq!(found.email, "test@example.com");
        assert_eq!(found.short.len(), 7);
        assert!(found.id.starts_with(&found.short));
        assert!(found.when > 0);
        assert!(
            found.parents.is_empty(),
            "the first commit came from nothing"
        );
    }

    #[test]
    fn a_merge_has_two_parents() {
        let temp = Temp::new("log-merge");
        temp.commit("a.txt", "one\n", "first");
        temp.run(&["checkout", "-b", "side"]);
        temp.commit("b.txt", "side\n", "on the side");
        temp.run(&["checkout", "main"]);
        temp.commit("c.txt", "main\n", "on main");
        temp.run(&["merge", "--no-ff", "side", "-m", "merged"]);

        let found = log(&temp.repo(), None, 10).expect("it walks");
        let merge = found
            .iter()
            .find(|one| one.summary == "merged")
            .expect("it is there");
        assert!(merge.is_merge());
        assert_eq!(merge.parents.len(), 2);
    }

    #[test]
    fn a_branch_can_be_walked_by_name() {
        let temp = Temp::new("log-branch");
        temp.commit("a.txt", "one\n", "first");
        temp.run(&["checkout", "-b", "side"]);
        temp.commit("b.txt", "side\n", "on the side");
        temp.run(&["checkout", "main"]);

        let side = log(&temp.repo(), Some("side"), 10).expect("it walks");
        assert_eq!(side[0].summary, "on the side");
        let main = log(&temp.repo(), Some("main"), 10).expect("it walks");
        assert_eq!(main[0].summary, "first", "main never saw the side branch");
    }
}
