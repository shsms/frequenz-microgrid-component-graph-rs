// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the `Error` struct and the `ErrorKind` enum, which are
//! used to represent errors that can occur in the library.

/// A macro for defining the `ErrorKind` enum, the `Display` implementation for
/// it, and the constructors for the `Error` struct.
macro_rules! ErrorKind {
    ($(
        ($kind:ident, $ctor:ident)
    ),*) => {
        /// The kind of error that occurred.
        #[derive(Debug, PartialEq)]
        pub(crate) enum ErrorKind {
            $(
                $kind,
            )*
        }

        impl std::fmt::Display for ErrorKind {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        Self::$kind => write!(f, "{}", stringify!($kind)),
                    )*
                }
            }
        }

        /// Constructors for [`Error`].
        impl Error {
            $(
                #[doc = concat!(
                    "Creates a new [`Error`] with the `",
                    stringify!($kind),
                    "` kind and the given description."
                )]
                pub(crate) fn $ctor(desc: impl Into<String>) -> crate::Error {
                    Self {
                        kind: ErrorKind::$kind,
                        desc: desc.into(),
                    }
                }
            )*
        }
    };
}

ErrorKind!(
    (ComponentNotFound, component_not_found),
    (Internal, internal),
    (InvalidComponent, invalid_component),
    (InvalidConnection, invalid_connection),
    (InvalidGraph, invalid_graph)
);

/// An error that can occur during the creation or traversal of a
/// [ComponentGraph][crate::ComponentGraph].
#[derive(Debug, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    desc: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.desc)
    }
}

impl std::error::Error for Error {}
