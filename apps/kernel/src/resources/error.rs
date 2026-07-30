use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceServiceErrorKind {
    InvalidCursor,
    InvalidPath,
    NotFound,
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

    pub(crate) const fn not_found() -> Self {
        Self {
            kind: ResourceServiceErrorKind::NotFound,
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
            ResourceServiceErrorKind::InvalidPath => "the resource path is invalid",
            ResourceServiceErrorKind::NotFound => "the resource was not found",
            ResourceServiceErrorKind::WrongKind => "the workspace entry is not a resource",
            ResourceServiceErrorKind::UnsafeTarget => "the resource target is unsafe",
            ResourceServiceErrorKind::Unavailable => "the resource service is unavailable",
        })
    }
}

impl std::error::Error for ResourceServiceError {}
