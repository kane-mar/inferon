// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Prompt registry (ITEM-097): name a content hash, list its versions, diff two.
//!
//! Identity stays content-derived — the label is additive metadata on
//! `(tenant, name, hash)` — so unmanaged prompts keep working and naming one
//! retroactively is the whole feature. Versions are a view over traces ordered
//! by first-seen; nothing here rewrites history.
//!
//! v1 is read-only: no editing, no deployment. The diff is for the owner to see
//! what changed between two captured prompts, so it obeys the capture switch.

/// One line of a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Same,
    Added,
    Removed,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }
}

/// Line diff between two prompt versions (longest common subsequence).
///
/// Prompts are small; the bound keeps a pathological input from allocating a
/// quadratic table. Past the bound the caller gets a whole-text replacement,
/// which is honest about not having compared the lines.
pub const MAX_DIFF_LINES: usize = 2_000;

pub fn diff(previous: &str, current: &str) -> Vec<Change> {
    let before: Vec<&str> = previous.lines().collect();
    let after: Vec<&str> = current.lines().collect();
    if before.len() > MAX_DIFF_LINES || after.len() > MAX_DIFF_LINES {
        let mut changes: Vec<Change> = before
            .iter()
            .map(|line| Change { kind: Kind::Removed, text: (*line).to_string() })
            .collect();
        changes.extend(after.iter().map(|line| Change {
            kind: Kind::Added,
            text: (*line).to_string(),
        }));
        return changes;
    }

    // lcs[i][j] = LCS length of before[i..] and after[j..]
    let mut lcs = vec![vec![0usize; after.len() + 1]; before.len() + 1];
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            lcs[i][j] = if before[i] == after[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut changes = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < before.len() && j < after.len() {
        if before[i] == after[j] {
            changes.push(Change { kind: Kind::Same, text: before[i].to_string() });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            changes.push(Change { kind: Kind::Removed, text: before[i].to_string() });
            i += 1;
        } else {
            changes.push(Change { kind: Kind::Added, text: after[j].to_string() });
            j += 1;
        }
    }
    changes.extend(before[i..].iter().map(|line| Change {
        kind: Kind::Removed,
        text: (*line).to_string(),
    }));
    changes.extend(after[j..].iter().map(|line| Change {
        kind: Kind::Added,
        text: (*line).to_string(),
    }));
    changes
}

#[cfg(test)]
mod tests;
