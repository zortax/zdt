use super::{CONTEXT, DiffHunk, Line, LineKind};

/// A blob's bytes as lines, without their newlines.
pub(super) fn lines_of(bytes: &[u8]) -> Vec<String> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    // A file ending in a newline splits into a trailing empty piece that is not a line.
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Whether a blob is something with lines in it.
///
/// A zero byte in the first eight kilobytes, which is the rule git itself uses. This decides one
/// thing only: whether showing the blob as text would be a screenful of replacement characters.
pub(super) fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|byte| *byte == 0)
}

/// The hunks between two lists of lines.
pub(super) fn hunks_between(old: &[String], new: &[String]) -> Vec<DiffHunk> {
    let script = myers(old, new);
    if script.iter().all(|step| matches!(step, Step::Keep(_, _))) {
        return Vec::new();
    }

    // Group the script into runs of changes with up to `CONTEXT` unchanged lines around them,
    // joining two runs that are close enough that the context between them would overlap.
    let changed: Vec<usize> = script
        .iter()
        .enumerate()
        .filter(|(_, step)| !matches!(step, Step::Keep(_, _)))
        .map(|(at, _)| at)
        .collect();

    let mut groups: Vec<(usize, usize)> = Vec::new();
    for at in changed {
        let from = at.saturating_sub(CONTEXT as usize);
        let to = (at + CONTEXT as usize + 1).min(script.len());
        match groups.last_mut() {
            Some((_, end)) if *end >= from => *end = to.max(*end),
            _ => groups.push((from, to)),
        }
    }

    groups
        .into_iter()
        .map(|(from, to)| {
            let mut lines = Vec::new();
            let (mut old_start, mut new_start) = (0, 0);
            let (mut old_count, mut new_count) = (0, 0);
            let mut started = false;

            for step in &script[from..to] {
                let line = match step {
                    Step::Keep(o, n) => {
                        old_count += 1;
                        new_count += 1;
                        Line {
                            kind: LineKind::Context,
                            text: old[*o].clone(),
                            old: Some(*o as u32 + 1),
                            new: Some(*n as u32 + 1),
                        }
                    }
                    Step::Remove(o) => {
                        old_count += 1;
                        Line {
                            kind: LineKind::Removed,
                            text: old[*o].clone(),
                            old: Some(*o as u32 + 1),
                            new: None,
                        }
                    }
                    Step::Add(n) => {
                        new_count += 1;
                        Line {
                            kind: LineKind::Added,
                            text: new[*n].clone(),
                            old: None,
                            new: Some(*n as u32 + 1),
                        }
                    }
                };
                if !started {
                    old_start = line.old.unwrap_or(0);
                    new_start = line.new.unwrap_or(0);
                    started = true;
                }
                // A hunk that opens on an added line still starts somewhere in the old file, and
                // that somewhere is wherever the next old line is.
                if old_start == 0
                    && let Some(old) = line.old
                {
                    old_start = old;
                }
                if new_start == 0
                    && let Some(new) = line.new
                {
                    new_start = new;
                }
                lines.push(line);
            }

            DiffHunk {
                old_start: old_start.max(1),
                old_count,
                new_start: new_start.max(1),
                new_count,
                lines,
            }
        })
        .collect()
}

/// One step of an edit script.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// A line in both, at these two places.
    Keep(usize, usize),
    /// A line only in the old text.
    Remove(usize),
    /// A line only in the new text.
    Add(usize),
}

/// The edit script between two lists of lines.
///
/// A longest-common-subsequence diff, with the common prefix and suffix taken off first. That is
/// what makes it fast enough on real files. A one-line change in a two-thousand-line file leaves a
/// handful of lines for the quadratic part to work on.
fn myers(old: &[String], new: &[String]) -> Vec<Step> {
    let head = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let mut script: Vec<Step> = (0..head).map(|at| Step::Keep(at, at)).collect();

    let (old_middle, new_middle) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    script.extend(
        lcs(old_middle, new_middle)
            .into_iter()
            .map(|step| match step {
                Step::Keep(o, n) => Step::Keep(o + head, n + head),
                Step::Remove(o) => Step::Remove(o + head),
                Step::Add(n) => Step::Add(n + head),
            }),
    );

    for at in 0..tail {
        script.push(Step::Keep(old.len() - tail + at, new.len() - tail + at));
    }
    script
}

/// The edit script between two lists that share no prefix or suffix.
fn lcs(old: &[String], new: &[String]) -> Vec<Step> {
    if old.is_empty() {
        return (0..new.len()).map(Step::Add).collect();
    }
    if new.is_empty() {
        return (0..old.len()).map(Step::Remove).collect();
    }

    // A table of how long the common subsequence is from each pair of positions onward. Quadratic,
    // which is why the prefix and suffix are stripped before this is reached; a change large
    // enough for this to matter is one nobody is reading line by line anyway.
    let (rows, columns) = (old.len() + 1, new.len() + 1);
    let mut table = vec![0u32; rows * columns];
    for o in (0..old.len()).rev() {
        for n in (0..new.len()).rev() {
            table[o * columns + n] = if old[o] == new[n] {
                table[(o + 1) * columns + n + 1] + 1
            } else {
                table[(o + 1) * columns + n].max(table[o * columns + n + 1])
            };
        }
    }

    let mut script = Vec::new();
    let (mut o, mut n) = (0, 0);
    while o < old.len() && n < new.len() {
        if old[o] == new[n] {
            script.push(Step::Keep(o, n));
            o += 1;
            n += 1;
        } else if table[(o + 1) * columns + n] >= table[o * columns + n + 1] {
            script.push(Step::Remove(o));
            o += 1;
        } else {
            script.push(Step::Add(n));
            n += 1;
        }
    }
    while o < old.len() {
        script.push(Step::Remove(o));
        o += 1;
    }
    while n < new.len() {
        script.push(Step::Add(n));
        n += 1;
    }
    script
}
