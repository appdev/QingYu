pub(crate) fn protected_resource_component(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        ".qingyu"
            | ".markra-sync"
            | ".markraignore"
            | ".codex"
            | ".git"
            | ".obsidian"
            | "build"
            | "dist"
            | "node_modules"
            | "target"
    ) || lower.starts_with(".qingyu-")
        || lower.starts_with(".markra-sync-stage-")
}
