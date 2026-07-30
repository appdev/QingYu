//! Compatibility helpers for Kernel-owned workspace ignore rules.

use std::path::Path;

use cap_std::fs::Dir;

pub(crate) use qingyu_kernel::ignore_rules::MarkdownIgnoreRules;

pub(crate) fn open_retained_ignore_root(root: &Path) -> Result<Dir, String> {
    Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map_err(|_| "workspace ignore rules are unavailable".to_string())
}

pub(crate) fn try_markdown_ignore_rules_for_retained_root(
    root: &Path,
    retained_root: &Dir,
    global_rules: Option<&str>,
) -> Result<MarkdownIgnoreRules, String> {
    MarkdownIgnoreRules::try_for_retained_root(root, retained_root, global_rules)
        .map_err(|error| error.to_string())
}

pub(crate) fn try_markdown_ignore_rules_for_root(
    root: &Path,
    global_rules: Option<&str>,
) -> Result<MarkdownIgnoreRules, String> {
    let retained_root = open_retained_ignore_root(root)?;
    try_markdown_ignore_rules_for_retained_root(root, &retained_root, global_rules)
}
