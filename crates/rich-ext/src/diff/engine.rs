//! The diff engine: Myers' O(ND) algorithm in linear space, hunk grouping,
//! token-level diffs for intra-line emphasis and the `diff -u` text format.
//!
//! Elements are interned to integers first, a common prefix and suffix are
//! trimmed, and regions with no element in common are resolved without a
//! search, so large inputs with few changes stay fast. Past an edit cost of
//! [`COST_LIMIT`] the search splits at its furthest-reaching point instead of
//! the exact middle snake (git's heuristic): the script stays valid, it just
//! may not be minimal.

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Range;

/// The edit cost past which the middle-snake search takes a heuristic split.
pub const COST_LIMIT: usize = 1024;

/// One run of an edit script. Ranges index the compared sequences: lines for
/// [`diff_lines`], bytes of the input strings for [`diff_words`] and
/// [`diff_chars`]. The side a run does not touch has an empty range at the
/// position where the run applies.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// `old` and `new` hold equal elements.
    Equal {
        old: Range<usize>,
        new: Range<usize>,
    },
    /// `old` is removed; `new` is empty.
    Delete {
        old: Range<usize>,
        new: Range<usize>,
    },
    /// `new` is inserted; `old` is empty.
    Insert {
        old: Range<usize>,
        new: Range<usize>,
    },
}

impl Op {
    /// The range on the old side.
    pub fn old(&self) -> Range<usize> {
        match self {
            Op::Equal { old, .. } | Op::Delete { old, .. } | Op::Insert { old, .. } => old.clone(),
        }
    }
    /// The range on the new side.
    pub fn new_range(&self) -> Range<usize> {
        match self {
            Op::Equal { new, .. } | Op::Delete { new, .. } | Op::Insert { new, .. } => new.clone(),
        }
    }
    /// Whether this run is unchanged.
    pub fn is_equal(&self) -> bool {
        matches!(self, Op::Equal { .. })
    }
}

/// Diff two sequences of any hashable elements.
pub fn diff_slices<T: Hash + Eq>(old: &[T], new: &[T]) -> Vec<Op> {
    let mut ids: HashMap<&T, u32> = HashMap::new();
    let mut intern = |item| {
        let next = ids.len() as u32;
        *ids.entry(item).or_insert(next)
    };
    let a: Vec<u32> = old.iter().map(&mut intern).collect();
    let b: Vec<u32> = new.iter().map(&mut intern).collect();
    let mut raw = Vec::new();
    let max = (a.len() + b.len()).div_ceil(2) + 1;
    let mut vf = vec![0usize; 2 * max + 2];
    let mut vb = vec![0usize; 2 * max + 2];
    conquer(&a, 0..a.len(), &b, 0..b.len(), &mut vf, &mut vb, &mut raw);
    normalize(raw)
}

/// Diff two sequences of lines. Include line terminators in the elements when
/// a missing final newline should count as a change, as `diff` does.
pub fn diff_lines(old: &[&str], new: &[&str]) -> Vec<Op> {
    diff_slices(old, new)
}

/// Split `text` into word, whitespace and punctuation tokens, as byte ranges.
/// A word is a run of alphanumerics and `_`; whitespace runs are one token;
/// every other character is its own token.
pub fn tokenize(text: &str) -> Vec<Range<usize>> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut last: Option<Class> = None;
    for (i, c) in text.char_indices() {
        let k = class(c);
        let joins = k != Class::Other && last.as_ref() == Some(&k);
        match out.last_mut() {
            Some(range) if joins => range.end = i + c.len_utf8(),
            _ => out.push(i..i + c.len_utf8()),
        }
        last = Some(k);
    }
    out
}

fn diff_tokens(old: &str, new: &str, a: Vec<Range<usize>>, b: Vec<Range<usize>>) -> Vec<Op> {
    let at = |tokens: &[Range<usize>], text: &str, i: usize| {
        tokens.get(i).map_or(text.len(), |range| range.start)
    };
    let old_items: Vec<&str> = a.iter().map(|r| &old[r.clone()]).collect();
    let new_items: Vec<&str> = b.iter().map(|r| &new[r.clone()]).collect();
    diff_slices(&old_items, &new_items)
        .into_iter()
        .map(|op| {
            let (o, n) = (op.old(), op.new_range());
            let old = at(&a, old, o.start)..at(&a, old, o.end);
            let new = at(&b, new, n.start)..at(&b, new, n.end);
            match op {
                Op::Equal { .. } => Op::Equal { old, new },
                Op::Delete { .. } => Op::Delete { old, new },
                Op::Insert { .. } => Op::Insert { old, new },
            }
        })
        .collect()
}

/// Diff two strings by [`tokenize`]d words; ranges are byte offsets.
pub fn diff_words(old: &str, new: &str) -> Vec<Op> {
    diff_tokens(old, new, tokenize(old), tokenize(new))
}

/// Diff two strings character by character; ranges are byte offsets.
pub fn diff_chars(old: &str, new: &str) -> Vec<Op> {
    let chars = |s: &str| {
        s.char_indices()
            .map(|(i, c)| i..i + c.len_utf8())
            .collect::<Vec<_>>()
    };
    diff_tokens(old, new, chars(old), chars(new))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tag {
    Equal,
    Delete,
    Insert,
}

type Raw = (Tag, Range<usize>, Range<usize>);

fn push(out: &mut Vec<Raw>, tag: Tag, old: Range<usize>, new: Range<usize>) {
    if old.is_empty() && new.is_empty() {
        return;
    }
    out.push((tag, old, new));
}

fn conquer(
    a: &[u32],
    mut ar: Range<usize>,
    b: &[u32],
    mut br: Range<usize>,
    vf: &mut [usize],
    vb: &mut [usize],
    out: &mut Vec<Raw>,
) {
    // Common prefix.
    let prefix = a[ar.clone()]
        .iter()
        .zip(&b[br.clone()])
        .take_while(|(x, y)| x == y)
        .count();
    push(
        out,
        Tag::Equal,
        ar.start..ar.start + prefix,
        br.start..br.start + prefix,
    );
    ar.start += prefix;
    br.start += prefix;
    // Common suffix, emitted after the middle.
    let suffix = a[ar.clone()]
        .iter()
        .rev()
        .zip(b[br.clone()].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let tail = (ar.end - suffix..ar.end, br.end - suffix..br.end);
    ar.end -= suffix;
    br.end -= suffix;

    // An empty side, or no element in common: delete all, insert all.
    if ar.is_empty() || br.is_empty() || disjoint(&a[ar.clone()], &b[br.clone()]) {
        push(out, Tag::Delete, ar.clone(), br.start..br.start);
        push(out, Tag::Insert, ar.end..ar.end, br.clone());
    } else {
        match middle_snake(a, ar.clone(), b, br.clone(), vf, vb) {
            Some((x, y)) if (x, y) != (ar.start, br.start) && (x, y) != (ar.end, br.end) => {
                conquer(a, ar.start..x, b, br.start..y, vf, vb, out);
                conquer(a, x..ar.end, b, y..br.end, vf, vb, out);
            }
            _ => {
                push(out, Tag::Delete, ar.clone(), br.start..br.start);
                push(out, Tag::Insert, ar.end..ar.end, br.clone());
            }
        }
    }
    push(out, Tag::Equal, tail.0, tail.1);
}

/// True when no element of `a` occurs in `b`.
fn disjoint(a: &[u32], b: &[u32]) -> bool {
    if a.len() * b.len() <= 64 {
        return !a.iter().any(|x| b.contains(x));
    }
    let set: std::collections::HashSet<u32> = b.iter().copied().collect();
    !a.iter().any(|x| set.contains(x))
}

/// The start of the middle snake of the shortest edit script between
/// `a[ar]` and `b[br]`, in absolute indices. Both ranges are non-empty and
/// differ at both ends.
fn middle_snake(
    a: &[u32],
    ar: Range<usize>,
    b: &[u32],
    br: Range<usize>,
    vf: &mut [usize],
    vb: &mut [usize],
) -> Option<(usize, usize)> {
    let (n, m) = (ar.len() as isize, br.len() as isize);
    let a = &a[ar.clone()];
    let b = &b[br.clone()];
    let delta = n - m;
    let odd = delta & 1 == 1;
    let max = (n + m + 1) / 2;
    let offset = max + 1;
    let idx = |k: isize| (k + offset) as usize;
    vf[idx(1)] = 0;
    vb[idx(1)] = 0;
    for d in 0..=max {
        // Forward: furthest x on each diagonal k = x - y.
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && vf[idx(k - 1)] < vf[idx(k + 1)]) {
                vf[idx(k + 1)] as isize
            } else {
                vf[idx(k - 1)] as isize + 1
            };
            let mut y = x - k;
            let (x0, y0) = (x, y);
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            vf[idx(k)] = x as usize;
            let c = delta - k;
            if odd && c >= -(d - 1) && c < d && x + vb[idx(c)] as isize >= n {
                return Some((ar.start + x0 as usize, br.start + y0 as usize));
            }
            k += 2;
        }
        // Backward, in reversed coordinates.
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && vb[idx(k - 1)] < vb[idx(k + 1)]) {
                vb[idx(k + 1)] as isize
            } else {
                vb[idx(k - 1)] as isize + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[(n - x - 1) as usize] == b[(m - y - 1) as usize] {
                x += 1;
                y += 1;
            }
            vb[idx(k)] = x as usize;
            let c = delta - k;
            if !odd && c >= -d && c <= d && x + vf[idx(c)] as isize >= n {
                return Some((ar.start + (n - x) as usize, br.start + (m - y) as usize));
            }
            k += 2;
        }
        if d as usize >= COST_LIMIT {
            // Too expensive: split at the furthest forward point reached.
            let mut best: Option<(isize, isize)> = None;
            let mut k = -d;
            while k <= d {
                let x = (vf[idx(k)] as isize).min(n);
                let y = x - k;
                if (0..=m).contains(&y)
                    && x + y > 0
                    && x + y < n + m
                    && best.is_none_or(|(bx, by)| x + y > bx + by)
                {
                    best = Some((x, y));
                }
                k += 2;
            }
            return best.map(|(x, y)| (ar.start + x as usize, br.start + y as usize));
        }
    }
    None
}

/// Merge adjacent runs of one kind and put deletions before insertions within
/// every change block.
fn normalize(raw: Vec<Raw>) -> Vec<Op> {
    let mut merged: Vec<Raw> = Vec::new();
    let mut pending_delete: Option<Raw> = None;
    let mut pending_insert: Option<Raw> = None;
    let flush = |merged: &mut Vec<Raw>, d: &mut Option<Raw>, i: &mut Option<Raw>| {
        if let Some(d) = d.take() {
            merged.push(d);
        }
        if let Some(mut i) = i.take() {
            // After the deletions, inserts apply at the end of the deleted run.
            if let Some(last) = merged.last() {
                if last.0 == Tag::Delete {
                    i.1 = last.1.end..last.1.end;
                }
            }
            merged.push(i);
        }
    };
    for (tag, old, new) in raw {
        match tag {
            Tag::Equal => {
                flush(&mut merged, &mut pending_delete, &mut pending_insert);
                match merged.last_mut() {
                    Some(last) if last.0 == Tag::Equal => {
                        last.1.end = old.end;
                        last.2.end = new.end;
                    }
                    _ => merged.push((tag, old, new)),
                }
            }
            Tag::Delete => match &mut pending_delete {
                Some(d) => d.1.end = old.end,
                None => {
                    // A delete that follows inserts applies at the new side's
                    // start of the block.
                    let new_at = pending_insert.as_ref().map_or(new.start, |i| i.2.start);
                    pending_delete = Some((tag, old, new_at..new_at));
                }
            },
            Tag::Insert => match &mut pending_insert {
                Some(i) => i.2.end = new.end,
                None => pending_insert = Some((tag, old, new)),
            },
        }
    }
    flush(&mut merged, &mut pending_delete, &mut pending_insert);
    merged
        .into_iter()
        .map(|(tag, old, new)| match tag {
            Tag::Equal => Op::Equal { old, new },
            Tag::Delete => Op::Delete { old, new },
            Tag::Insert => Op::Insert { old, new },
        })
        .collect()
}

/// A group of changes with surrounding context, as `diff -u` prints them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The runs in this hunk, context included.
    pub ops: Vec<Op>,
}

impl Hunk {
    /// The old-side line range (0-based).
    pub fn old_range(&self) -> Range<usize> {
        let first = self.ops.first().map_or(0, |op| op.old().start);
        let last = self.ops.last().map_or(first, |op| op.old().end);
        first..last
    }
    /// The new-side line range (0-based).
    pub fn new_range(&self) -> Range<usize> {
        let first = self.ops.first().map_or(0, |op| op.new_range().start);
        let last = self.ops.last().map_or(first, |op| op.new_range().end);
        first..last
    }
    /// The `@@ -a,b +c,d @@` header, in `diff -u`'s shape.
    pub fn header(&self) -> String {
        hunk_header(self.old_range(), self.new_range())
    }
}

/// `@@ -a,b +c,d @@` for 0-based line ranges, with `diff -u`'s conventions:
/// a single line omits `,1`, and an empty range names the line before it.
pub fn hunk_header(old: Range<usize>, new: Range<usize>) -> String {
    fn part(r: Range<usize>) -> String {
        match r.len() {
            0 => format!("{},0", r.start),
            1 => format!("{}", r.start + 1),
            n => format!("{},{n}", r.start + 1),
        }
    }
    format!("@@ -{} +{} @@", part(old), part(new))
}

/// Group an edit script into hunks with `context` unchanged elements around
/// every change; changes closer than twice the context share a hunk.
pub fn group_hunks(ops: &[Op], context: usize) -> Vec<Hunk> {
    let spans: Vec<(bool, Range<usize>, Range<usize>)> = ops
        .iter()
        .map(|op| (op.is_equal(), op.old(), op.new_range()))
        .collect();
    group_ranges(&spans, context)
        .into_iter()
        .map(|group| Hunk {
            ops: group
                .into_iter()
                .map(|(index, old, new)| match &ops[index] {
                    Op::Equal { .. } => Op::Equal { old, new },
                    Op::Delete { .. } => Op::Delete { old, new },
                    Op::Insert { .. } => Op::Insert { old, new },
                })
                .collect(),
        })
        .collect()
}

/// A run in a hunk: its source index and trimmed old and new ranges.
pub(crate) type Grouped = (usize, Range<usize>, Range<usize>);

/// Hunk grouping over `(is_equal, old, new)` runs; returns, per hunk, the
/// index of each source run and its trimmed ranges.
pub(crate) fn group_ranges(
    runs: &[(bool, Range<usize>, Range<usize>)],
    context: usize,
) -> Vec<Vec<Grouped>> {
    // difflib's `get_grouped_opcodes`.
    if runs.iter().all(|r| r.0) {
        return Vec::new();
    }
    let mut codes: Vec<Grouped> = runs
        .iter()
        .enumerate()
        .map(|(i, (_, a, b))| (i, a.clone(), b.clone()))
        .collect();
    if let Some(first) = codes.first_mut() {
        if runs[first.0].0 {
            first.1.start = first.1.start.max(first.1.end.saturating_sub(context));
            first.2.start = first.2.start.max(first.2.end.saturating_sub(context));
        }
    }
    if let Some(last) = codes.last_mut() {
        if runs[last.0].0 {
            last.1.end = last.1.end.min(last.1.start + context);
            last.2.end = last.2.end.min(last.2.start + context);
        }
    }
    let mut hunks = Vec::new();
    let mut group: Vec<Grouped> = Vec::new();
    for (i, mut a, mut b) in codes {
        if runs[i].0 && a.len() > context * 2 {
            group.push((i, a.start..a.start + context, b.start..b.start + context));
            hunks.push(std::mem::take(&mut group));
            a.start = a.end - context;
            b.start = b.end - context;
        }
        group.push((i, a, b));
    }
    if group.iter().any(|(i, _, _)| !runs[*i].0) {
        hunks.push(group);
    }
    for hunk in &mut hunks {
        hunk.retain(|(i, a, b)| !(runs[*i].0 && a.is_empty() && b.is_empty()));
    }
    hunks.retain(|hunk| hunk.iter().any(|(i, _, _)| !runs[*i].0));
    hunks
}

/// Split text into lines that keep their `\n`; the last line may lack one.
pub(crate) fn split_lines_inclusive(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A line without its terminator (`\n` or `\r\n`).
pub(crate) fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// A line-level diff of two texts.
///
/// ```
/// use rich_ext::diff::TextDiff;
///
/// let diff = TextDiff::new("a\nb\nc\n", "a\nB\nc\n");
/// assert_eq!(diff.stats(), (1, 1));
/// assert_eq!(
///     diff.unified("old", "new"),
///     "--- old\n+++ new\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
/// );
/// ```
#[derive(Clone, Debug)]
pub struct TextDiff {
    old: Vec<String>,
    new: Vec<String>,
    ops: Vec<Op>,
    context: usize,
}

impl TextDiff {
    /// Diff `old` against `new` line by line. A line's terminator is part of
    /// it, so a missing final newline is a change.
    pub fn new(old: &str, new: &str) -> Self {
        let a = split_lines_inclusive(old);
        let b = split_lines_inclusive(new);
        let ops = diff_lines(&a, &b);
        TextDiff {
            old: a.into_iter().map(str::to_owned).collect(),
            new: b.into_iter().map(str::to_owned).collect(),
            ops,
            context: 3,
        }
    }
    /// Unchanged lines shown around each change (default 3).
    pub fn context(mut self, lines: usize) -> Self {
        self.context = lines;
        self
    }
    /// The configured context.
    pub fn context_lines(&self) -> usize {
        self.context
    }
    /// The edit script over lines.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }
    /// The old side's lines, terminators included.
    pub fn old_lines(&self) -> &[String] {
        &self.old
    }
    /// The new side's lines, terminators included.
    pub fn new_lines(&self) -> &[String] {
        &self.new
    }
    /// The hunks at the configured context.
    pub fn hunks(&self) -> Vec<Hunk> {
        group_hunks(&self.ops, self.context)
    }
    /// Whether both sides are identical.
    pub fn is_equal(&self) -> bool {
        self.ops.iter().all(Op::is_equal)
    }
    /// `(added, removed)` line counts.
    pub fn stats(&self) -> (usize, usize) {
        self.ops.iter().fold((0, 0), |(a, r), op| match op {
            Op::Insert { new, .. } => (a + new.len(), r),
            Op::Delete { old, .. } => (a, r + old.len()),
            Op::Equal { .. } => (a, r),
        })
    }
    /// The diff in `diff -u` format with `old_name`/`new_name` headers (no
    /// timestamps). Empty when the texts are equal, as `diff` prints nothing.
    pub fn unified(&self, old_name: &str, new_name: &str) -> String {
        let hunks = self.hunks();
        if hunks.is_empty() {
            return String::new();
        }
        let mut out = format!("--- {old_name}\n+++ {new_name}\n");
        let line = |out: &mut String, prefix: char, text: &str| {
            out.push(prefix);
            out.push_str(text);
            if !text.ends_with('\n') {
                out.push_str("\n\\ No newline at end of file\n");
            }
        };
        for hunk in &hunks {
            out.push_str(&hunk.header());
            out.push('\n');
            for op in &hunk.ops {
                match op {
                    Op::Equal { old, .. } => {
                        for text in &self.old[old.clone()] {
                            line(&mut out, ' ', text);
                        }
                    }
                    Op::Delete { old, .. } => {
                        for text in &self.old[old.clone()] {
                            line(&mut out, '-', text);
                        }
                    }
                    Op::Insert { new, .. } => {
                        for text in &self.new[new.clone()] {
                            line(&mut out, '+', text);
                        }
                    }
                }
            }
        }
        out
    }
}
