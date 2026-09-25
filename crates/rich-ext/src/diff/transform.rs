//! Transforms over a parsed git [`Patch`], for a
//! [`Pipeline`](crate::transform::Pipeline).

use super::git::Patch;
use crate::env_inspect::name_matches;
use crate::transform::{Transform, TransformError};

/// Keeps the files whose old or new path matches any pattern: a
/// case-insensitive substring, or a whole-path glob with `*` and `?`.
#[derive(Clone, Debug)]
pub struct KeepFiles(pub Vec<String>);

impl Transform<Patch> for KeepFiles {
    fn apply(&self, mut patch: Patch) -> Result<Patch, TransformError> {
        patch.files.retain(|file| {
            [&file.old_path, &file.new_path]
                .into_iter()
                .flatten()
                .any(|path| self.0.iter().any(|pattern| name_matches(pattern, path)))
        });
        Ok(patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::git::parse_unified;

    #[test]
    fn keeps_matching_files() {
        let patch = parse_unified(
            "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-a\n+b\n\
             diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-a\n+b\n",
        )
        .unwrap();
        let kept = KeepFiles(vec!["*.rs".into()]).apply(patch.clone()).unwrap();
        assert_eq!(kept.files.len(), 1);
        assert_eq!(kept.files[0].path(), "src/a.rs");
        assert_eq!(kept.stats(), (1, 1));
        let readme = KeepFiles(vec!["readme".into()]).apply(patch).unwrap();
        assert_eq!(readme.files[0].path(), "README.md");
    }
}
