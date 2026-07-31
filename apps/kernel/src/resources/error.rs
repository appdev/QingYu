use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceServiceErrorKind {
    InvalidCursor,
    InvalidMediaType,
    InvalidPath,
    NotFound,
    StaleWorkspace,
    TooLarge,
    WrongKind,
    UnsafeTarget,
    Unavailable,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ResourceServiceError {
    kind: ResourceServiceErrorKind,
}

impl ResourceServiceError {
    pub(crate) const fn invalid_cursor() -> Self {
        Self {
            kind: ResourceServiceErrorKind::InvalidCursor,
        }
    }

    pub(crate) const fn invalid_path() -> Self {
        Self {
            kind: ResourceServiceErrorKind::InvalidPath,
        }
    }

    pub(crate) const fn invalid_media_type() -> Self {
        Self {
            kind: ResourceServiceErrorKind::InvalidMediaType,
        }
    }

    pub(crate) const fn not_found() -> Self {
        Self {
            kind: ResourceServiceErrorKind::NotFound,
        }
    }

    pub(crate) const fn stale_workspace() -> Self {
        Self {
            kind: ResourceServiceErrorKind::StaleWorkspace,
        }
    }

    pub(crate) const fn too_large() -> Self {
        Self {
            kind: ResourceServiceErrorKind::TooLarge,
        }
    }

    pub(crate) const fn wrong_kind() -> Self {
        Self {
            kind: ResourceServiceErrorKind::WrongKind,
        }
    }

    pub(crate) const fn unsafe_target() -> Self {
        Self {
            kind: ResourceServiceErrorKind::UnsafeTarget,
        }
    }

    pub(crate) const fn unavailable() -> Self {
        Self {
            kind: ResourceServiceErrorKind::Unavailable,
        }
    }

    pub const fn kind(self) -> ResourceServiceErrorKind {
        self.kind
    }
}

impl fmt::Debug for ResourceServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceServiceError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ResourceServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            ResourceServiceErrorKind::InvalidCursor => "the resource cursor is invalid",
            ResourceServiceErrorKind::InvalidMediaType => "the resource media type is invalid",
            ResourceServiceErrorKind::InvalidPath => "the resource path is invalid",
            ResourceServiceErrorKind::NotFound => "the resource was not found",
            ResourceServiceErrorKind::StaleWorkspace => "the workspace generation is stale",
            ResourceServiceErrorKind::TooLarge => "the resource exceeds the supported size",
            ResourceServiceErrorKind::WrongKind => "the workspace entry is not a resource",
            ResourceServiceErrorKind::UnsafeTarget => "the resource target is unsafe",
            ResourceServiceErrorKind::Unavailable => "the resource service is unavailable",
        })
    }
}

impl std::error::Error for ResourceServiceError {}
