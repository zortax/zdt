//! Branches, and which one is checked out.

use crate::repo::{Error, Repo};

/// One branch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Branch {
    /// What it is called, without the `refs/heads/` in front.
    pub name: String,
    /// Where it points.
    pub id: String,
    /// Whether it is the one checked out.
    pub current: bool,
    /// Which remote branch it follows, when it follows one.
    pub upstream: Option<String>,
    /// Whether it is a remote branch rather than a local one.
    pub remote: bool,
}

/// Where `HEAD` is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Head {
    /// On a branch, by name.
    Branch(String),
    /// Not on one, at a commit.
    Detached(String),
    /// A repository with no commits in it, so `HEAD` points at a branch that does not exist yet.
    Unborn(String),
}

impl Head {
    /// What to put in the corner of the window.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Branch(name) | Self::Unborn(name) => name.clone(),
            // Seven characters and a mark, which is what git itself shows and what tells somebody
            // at a glance that they are not on a branch.
            Self::Detached(id) => format!("@{}", id.chars().take(7).collect::<String>()),
        }
    }

    /// The branch's name, when there is one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Branch(name) | Self::Unborn(name) => Some(name),
            Self::Detached(_) => None,
        }
    }
}

/// Where `HEAD` is.
///
/// # Errors
///
/// When `HEAD` cannot be read at all, which means the repository is broken rather than empty.
pub fn head(repo: &Repo) -> Result<Head, Error> {
    let found = repo.git().head().map_err(Error::git)?;

    if let Some(name) = found.referent_name() {
        let short = name.shorten().to_string();
        return Ok(if found.id().is_some() {
            Head::Branch(short)
        } else {
            // The name is written but nothing is at it yet: a repository somebody has just run
            // `git init` in.
            Head::Unborn(short)
        });
    }
    match found.id() {
        Some(id) => Ok(Head::Detached(id.to_string())),
        None => Ok(Head::Unborn("main".to_owned())),
    }
}

/// Every branch, local ones first and each in name order.
///
/// Remote branches are included because half of what somebody wants a branch list for is to see
/// whether the thing they are about to push already exists.
///
/// # Errors
///
/// When the references cannot be read.
pub fn branches(repo: &Repo) -> Result<Vec<Branch>, Error> {
    let git = repo.git();
    let current = head(repo)
        .ok()
        .and_then(|head| head.name().map(str::to_owned));

    let mut local: Vec<Branch> = Vec::new();
    let mut remote: Vec<Branch> = Vec::new();

    let platform = git.references().map_err(Error::git)?;
    for found in platform.all().map_err(Error::git)? {
        let Ok(mut reference) = found else {
            continue;
        };
        let full = reference.name().as_bstr().to_string();
        let is_remote = full.starts_with("refs/remotes/");
        if !is_remote && !full.starts_with("refs/heads/") {
            continue;
        }
        // `origin/HEAD` is a pointer to another branch rather than a branch, and listing it puts
        // the same commit in the list twice under two names.
        if full.ends_with("/HEAD") {
            continue;
        }

        let name = reference.name().shorten().to_string();
        let Ok(id) = reference.peel_to_id() else {
            continue;
        };
        let upstream = (!is_remote).then(|| upstream_of(git, &name)).flatten();

        let branch = Branch {
            current: !is_remote && current.as_deref() == Some(name.as_str()),
            name,
            id: id.detach().to_string(),
            upstream,
            remote: is_remote,
        };
        if is_remote {
            remote.push(branch)
        } else {
            local.push(branch)
        }
    }

    local.sort_by(|a, b| a.name.cmp(&b.name));
    remote.sort_by(|a, b| a.name.cmp(&b.name));
    local.extend(remote);
    Ok(local)
}

/// Which remote branch a local one follows, as its configuration says.
fn upstream_of(git: &gix::Repository, name: &str) -> Option<String> {
    let config = git.config_snapshot();
    let remote = config.string(format!("branch.{name}.remote").as_str())?;
    let merge = config.string(format!("branch.{name}.merge").as_str())?;
    let branch = merge.to_string();
    let branch = branch.strip_prefix("refs/heads/").unwrap_or(&branch);
    Some(format!("{remote}/{branch}"))
}

#[cfg(test)]
mod tests {
    use super::{Head, branches, head};
    use crate::repo::testing::Temp;

    #[test]
    fn the_branch_that_is_checked_out_says_so() {
        let temp = Temp::new("refs-current");
        temp.commit("a.txt", "one\n", "first");
        temp.run(&["checkout", "-b", "side"]);

        assert_eq!(
            head(&temp.repo()).expect("it reads"),
            Head::Branch("side".to_owned())
        );

        let found = branches(&temp.repo()).expect("it lists");
        let side = found
            .iter()
            .find(|one| one.name == "side")
            .expect("it is there");
        assert!(side.current);
        let main = found
            .iter()
            .find(|one| one.name == "main")
            .expect("it is there");
        assert!(!main.current);
    }

    #[test]
    fn a_repository_with_no_commits_has_an_unborn_head() {
        // Which is a project somebody has just run `git init` in, and the panel opens on it.
        let temp = Temp::new("refs-unborn");
        assert!(matches!(
            head(&temp.repo()).expect("it reads"),
            Head::Unborn(_)
        ));
        assert!(branches(&temp.repo()).expect("it lists").is_empty());
    }

    #[test]
    fn a_detached_head_reads_as_a_commit() {
        let temp = Temp::new("refs-detached");
        temp.commit("a.txt", "one\n", "first");
        temp.commit("b.txt", "two\n", "second");
        temp.run(&["checkout", "--detach", "HEAD~1"]);

        let found = head(&temp.repo()).expect("it reads");
        assert!(matches!(found, Head::Detached(_)));
        assert!(found.name().is_none());
        assert!(found.label().starts_with('@'), "{}", found.label());
        assert_eq!(found.label().len(), 8, "a mark and seven characters");
    }

    #[test]
    fn branches_come_back_in_name_order_with_the_local_ones_first() {
        let temp = Temp::new("refs-order");
        temp.commit("a.txt", "one\n", "first");
        for name in ["zebra", "apple", "mango"] {
            temp.run(&["branch", name]);
        }

        let found = branches(&temp.repo()).expect("it lists");
        let names: Vec<&str> = found.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["apple", "main", "mango", "zebra"]);
        assert!(
            found.iter().all(|one| !one.remote),
            "there is no remote here"
        );
    }

    #[test]
    fn every_branch_points_at_something() {
        let temp = Temp::new("refs-ids");
        temp.commit("a.txt", "one\n", "first");
        temp.run(&["branch", "side"]);

        for branch in branches(&temp.repo()).expect("it lists") {
            assert_eq!(branch.id.len(), 40, "{} has no commit", branch.name);
        }
    }
}
