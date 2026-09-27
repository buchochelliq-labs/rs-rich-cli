//! Fuzzy matching for pickers: a pattern's characters in order, anywhere in
//! the candidate, scored so that the matches people mean rank first.
//!
//! A match scores for each character matched, more at the start of a word
//! (after a space, `/`, `_`, `-`, `.` or a lower-to-upper case change) and
//! for runs of consecutive characters, and loses a little for each gap. The
//! best-scoring alignment is found by dynamic programming, so `fb` in
//! `foo_bar` highlights `f` and `b`, not `f` and the `b` of a later word.
//! Case is ignored unless the pattern has an upper-case letter (smart case).
//! Space-separated terms must all match, in any order.

/// A candidate that matched: its score, and the character positions that
/// matched (for highlighting), in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub score: i64,
    pub positions: Vec<usize>,
}

const MATCH: i64 = 16;
const BOUNDARY: i64 = 8;
const CAMEL: i64 = 7;
const CONSECUTIVE: i64 = 8;
/// A gap costs this, plus one for each character skipped after the first.
const GAP_START: i64 = 3;

fn separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '/' | '\\' | '_' | '-' | '.' | ':' | ',' | '(' | '[')
}

/// The bonus for matching at `index`, from the character before it.
fn bonus(chars: &[char], index: usize) -> i64 {
    match index.checked_sub(1).map(|previous| chars[previous]) {
        None => BOUNDARY,
        Some(previous) if separator(previous) => BOUNDARY,
        Some(previous) if previous.is_lowercase() && chars[index].is_uppercase() => CAMEL,
        _ => 0,
    }
}

/// Score one term against the candidate.
fn term(pattern: &[char], candidate: &[char], fold: bool) -> Option<Match> {
    let (m, n) = (pattern.len(), candidate.len());
    if m == 0 {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    if m > n {
        return None;
    }
    let same = |p: char, c: char| {
        if fold {
            c.to_lowercase().eq(p.to_lowercase())
        } else {
            p == c
        }
    };
    const NONE: i64 = i64::MIN / 4;
    // score[i][j]: best score with pattern[i] matched at candidate[j];
    // from[i][j]: where pattern[i - 1] was matched for that score.
    let mut score = vec![NONE; m * n];
    let mut from = vec![usize::MAX; m * n];
    for j in 0..n {
        if same(pattern[0], candidate[j]) {
            score[j] = MATCH + bonus(candidate, j);
        }
    }
    for i in 1..m {
        // The best of score[i - 1][k] + k over k <= j - 2, for gaps.
        let mut best: (i64, usize) = (NONE, usize::MAX);
        for j in 1..n {
            if j >= 2 {
                let k = j - 2;
                let value = score[(i - 1) * n + k];
                if value > NONE && value + k as i64 > best.0 {
                    best = (value + k as i64, k);
                }
            }
            if !same(pattern[i], candidate[j]) {
                continue;
            }
            let here = MATCH + bonus(candidate, j);
            let adjacent = score[(i - 1) * n + j - 1];
            let run = if adjacent > NONE {
                adjacent + CONSECUTIVE
            } else {
                NONE
            };
            // A gap from k to j costs GAP_START + (j - k - 2).
            let gap = if best.0 > NONE {
                best.0 - j as i64 - GAP_START + 2
            } else {
                NONE
            };
            let (value, previous) = if run >= gap {
                (run, j - 1)
            } else {
                (gap, best.1)
            };
            if value > NONE {
                score[i * n + j] = value + here;
                from[i * n + j] = previous;
            }
        }
    }
    let last = m - 1;
    let (end, best) = (0..n)
        .map(|j| (j, score[last * n + j]))
        .filter(|(_, value)| *value > NONE)
        .max_by_key(|&(j, value)| (value, std::cmp::Reverse(j)))?;
    let mut positions = vec![0; m];
    let mut j = end;
    for i in (0..m).rev() {
        positions[i] = j;
        if i > 0 {
            j = from[i * n + j];
        }
    }
    Some(Match {
        score: best,
        positions,
    })
}

/// Match `pattern` against `candidate`: every space-separated term must
/// match. An empty pattern matches everything with score 0.
pub fn fuzzy(pattern: &str, candidate: &str) -> Option<Match> {
    let candidate: Vec<char> = candidate.chars().collect();
    let mut total = Match {
        score: 0,
        positions: Vec::new(),
    };
    for word in pattern.split_whitespace() {
        let chars: Vec<char> = word.chars().collect();
        let fold = !chars.iter().any(|c| c.is_uppercase());
        let found = term(&chars, &candidate, fold)?;
        total.score += found.score;
        total.positions.extend(found.positions);
    }
    total.positions.sort_unstable();
    total.positions.dedup();
    Some(total)
}

/// Filter and rank `candidates` by `pattern`: matching indices, best
/// first; ties go to the shorter candidate, then to the earlier one.
pub fn rank<'a>(
    pattern: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Vec<(usize, Match)> {
    let mut found: Vec<(usize, usize, Match)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            fuzzy(pattern, candidate).map(|m| (index, candidate.chars().count(), m))
        })
        .collect();
    if !pattern.trim().is_empty() {
        found.sort_by(|a, b| {
            b.2.score
                .cmp(&a.2.score)
                .then(a.1.cmp(&b.1))
                .then(a.0.cmp(&b.0))
        });
    }
    found.into_iter().map(|(index, _, m)| (index, m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_characters_in_order() {
        assert!(fuzzy("abc", "a_b_c").is_some());
        assert!(fuzzy("acb", "abc").is_none());
        assert!(fuzzy("", "anything").unwrap().positions.is_empty());
        assert!(fuzzy("xyz", "xy").is_none());
    }

    #[test]
    fn prefers_word_starts_and_runs() {
        // `fb` takes the start of each word, not the first `b`.
        assert_eq!(fuzzy("fb", "fabric_bar").unwrap().positions, [0, 7]);
        assert_eq!(fuzzy("bar", "b_a_r bar").unwrap().positions, [6, 7, 8]);
        // camelCase humps count as word starts.
        assert_eq!(fuzzy("gc", "getConfig").unwrap().positions, [0, 3]);
    }

    #[test]
    fn smart_case() {
        assert!(fuzzy("readme", "README.md").is_some());
        assert!(fuzzy("Readme", "readme.md").is_none());
        assert!(fuzzy("README", "README.md").is_some());
    }

    #[test]
    fn every_term_must_match() {
        let found = fuzzy("main rs", "src/main.rs").unwrap();
        assert_eq!(found.positions, [4, 5, 6, 7, 9, 10]);
        assert!(fuzzy("main py", "src/main.rs").is_none());
    }

    #[test]
    fn ranks_the_intended_match_first() {
        let files = [
            "src/lib.rs",
            "src/main.rs",
            "docs/maintenance.md",
            "Cargo.toml",
        ];
        let ranked: Vec<usize> = rank("main", files).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ranked, [1, 2]);
        let ranked: Vec<usize> = rank("", files).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ranked, [0, 1, 2, 3], "no pattern keeps the order");
        let ranked: Vec<usize> = rank("rs", files).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ranked, [0, 1]);
    }
}
