// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the `Error` struct and the `ErrorKind` enum, which are
//! used to represent errors that can occur in the library.

/// The kind of an [`Error`].
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    /// No component was found for a given component ID.
    ComponentNotFound,

    /// An internal invariant of the library was violated. This indicates a bug.
    Internal,

    /// A component is invalid, e.g. it has an unspecified category.
    InvalidComponent,

    /// A connection between two components is invalid.
    InvalidConnection,

    /// The graph is structurally invalid, e.g. it has no grid component,
    /// several grid components, or a duplicate component ID.
    InvalidGraph,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::ComponentNotFound => "ComponentNotFound",
            Self::Internal => "Internal",
            Self::InvalidComponent => "InvalidComponent",
            Self::InvalidConnection => "InvalidConnection",
            Self::InvalidGraph => "InvalidGraph",
        };
        f.write_str(name)
    }
}

/// An error that can occur during the creation or traversal of a
/// [ComponentGraph][crate::ComponentGraph].
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    desc: String,
}

impl Error {
    /// Returns the [`ErrorKind`] of this error.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

/// Constructors for [`Error`].
impl Error {
    /// Creates a new [`Error`] with the `ComponentNotFound` kind and the given
    /// description.
    pub(crate) fn component_not_found(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ComponentNotFound,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `Internal` kind and the given
    /// description.
    pub(crate) fn internal(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `InvalidComponent` kind and the given
    /// description.
    pub(crate) fn invalid_component(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidComponent,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `InvalidConnection` kind and the given
    /// description.
    pub(crate) fn invalid_connection(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidConnection,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `InvalidGraph` kind and the given
    /// description.
    pub(crate) fn invalid_graph(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidGraph,
            desc: desc.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.desc)
    }
}

impl std::error::Error for Error {}
