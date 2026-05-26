// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

#![doc = include_str!("../README.md")]

mod component_category;
pub use component_category::{BatteryType, ComponentCategory, EvChargerType, InverterType};

mod graph;
pub use graph::{AggregationFormula, CoalesceFormula, ComponentGraph, Formula, iterators};

mod graph_traits;
pub use graph_traits::{Edge, Node};

mod error;
pub use error::{Error, ErrorKind};

mod config;
pub use config::{
    ComponentGraphConfig, ComponentGraphConfigBuilder, FormulaOverrides, FormulaOverridesBuilder,
};
