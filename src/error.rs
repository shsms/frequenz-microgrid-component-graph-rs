// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the `Error` struct and the `ErrorKind` enum, which are
//! used to represent errors that can occur in the library.

/// The kind of error that occurred.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ErrorKind {
    ComponentNotFound,
    Internal,
    InvalidComponent,
    InvalidConnection,
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
