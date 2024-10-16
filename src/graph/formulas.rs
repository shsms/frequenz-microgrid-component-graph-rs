// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

mod expr;
mod fallback;
mod generators;
mod traversal;

use crate::{ComponentGraph, Edge, Error, Node};
use std::collections::BTreeSet;

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    pub fn consumer_formula(&self) -> Result<String, Error> {
        generators::consumer::ConsumerFormulaBuilder::try_new(self)?.build()
    }

    pub fn grid_formula(&self) -> Result<String, Error> {
        generators::grid::GridFormulaBuilder::try_new(self)?.build()
    }

    pub fn producer_formula(&self) -> Result<String, Error> {
        generators::producer::ProducerFormulaBuilder::try_new(self)?.build()
    }

    pub fn battery_formula(&self, battery_ids: Option<BTreeSet<u64>>) -> Result<String, Error> {
        generators::battery::BatteryFormulaBuilder::try_new(self, battery_ids)?.build()
    }

    pub fn chp_formula(&self, chp_ids: Option<BTreeSet<u64>>) -> Result<String, Error> {
        generators::chp::CHPFormulaBuilder::try_new(self, chp_ids)?.build()
    }

    pub fn pv_formula(&self, pv_inverter_ids: Option<BTreeSet<u64>>) -> Result<String, Error> {
        generators::pv::PVFormulaBuilder::try_new(self, pv_inverter_ids)?.build()
    }

    pub fn ev_charger_formula(
        &self,
        ev_charger_ids: Option<BTreeSet<u64>>,
    ) -> Result<String, Error> {
        generators::ev_charger::EVChargerFormulaBuilder::try_new(self, ev_charger_ids)?.build()
    }
}
