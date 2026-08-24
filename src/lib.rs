// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![doc = include_str!("../README.md")]

mod component_category;
pub use component_category::{BatteryType, ComponentCategory, EvChargerType, InverterType};

mod operational_mode;
pub use operational_mode::OperationalMode;

mod graph;
pub use graph::{ComponentGraph, Formula, iterators};
#[cfg(feature = "explain")]
pub use graph::{ExplainedFormula, Explanation, ExplanationKind, FormulaAst};

mod graph_traits;
pub use graph_traits::{Edge, Node};

mod error;
pub use error::{Error, ErrorKind, ValidationError};

mod config;
pub use config::{
    ComponentGraphConfig, ComponentGraphConfigBuilder, FormulaOverrides, FormulaOverridesBuilder,
};
