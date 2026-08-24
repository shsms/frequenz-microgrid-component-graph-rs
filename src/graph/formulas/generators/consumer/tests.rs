// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Tests for the consumer formula generator.

use super::ConsumerFormulaBuilder;
use crate::graph::formulas::explain::ExplanationKind;
use crate::graph::test_utils::ComponentGraphBuilder;
use crate::{ComponentCategory, ComponentGraphConfig, Error, InverterType, OperationalMode};

#[test]
fn test_zero_consumers() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a battery inverter to the grid, without a battery meter.
    let inv_bat_chain = builder.inv_bat_chain(1);
    builder.connect(grid, inv_bat_chain);

    let graph = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "0.0");

    Ok(())
}

#[test]
fn test_consumer_formula_with_grid_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a grid meter to the grid, with no successors.
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config)?;
    let graph_no_phantom = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0)");
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0)");

    // Add a battery meter with one battery inverter and one battery to the
    // grid meter.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid_meter, meter_bat_chain);

    assert_eq!(meter_bat_chain.component_id(), 2);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    // Formula subtracts the battery meter from the grid meter, and the
    // battery inverter from the battery meter.
    assert_eq!(
        formula,
        "MAX(#1 - COALESCE(#2, #3, 0.0), 0.0) + COALESCE(MAX(#2 - #3, 0.0), 0.0)"
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1 - COALESCE(#2, #3, 0.0), 0.0)");

    // Add a solar meter with two solar inverters to the grid meter.
    let meter_pv_chain = builder.meter_pv_chain(2);
    builder.connect(grid_meter, meter_pv_chain);

    assert_eq!(meter_pv_chain.component_id(), 5);

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors
            "MAX(",
            "#1 - COALESCE(#2, #3, 0.0) - COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)), ",
            "0.0",
            ") + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0)",
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            "MAX(",
            "#1 - COALESCE(#2, #3, 0.0) - COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)), ",
            "0.0",
            ")",
        )
    );

    // Add a "mixed" meter with a CHP, an ev charger and a solar inverter to
    // the grid meter.
    let solar_inverter = builder.solar_inverter();
    let chp = builder.chp();
    let ev_charger = builder.ev_charger();
    let meter = builder.meter();
    builder.connect(meter, solar_inverter);
    builder.connect(meter, chp);
    builder.connect(meter, ev_charger);
    builder.connect(grid_meter, meter);

    assert_eq!(solar_inverter.component_id(), 8);
    assert_eq!(chp.component_id(), 9);
    assert_eq!(ev_charger.component_id(), 10);
    assert_eq!(meter.component_id(), 11);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors
            "MAX(",
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)), ",
            "0.0) + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
            // difference of "mixed" meter from its successors.
            "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all non-consumer meters
            "MAX(",
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)), ",
            "0.0)"
        )
    );

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors (without fallbacks)
            "MAX(#1 - #2 - #5 - #11, 0.0) + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
            // difference of "mixed" meter from its successors.
            "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0)"
        )
    );
    let graph_no_phantom = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .include_phantom_loads_in_consumer_formula(false)
            .build(),
    ))?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1 - #2 - #5 - #8 - #9 - #10, 0.0)");

    // add a battery chain to the grid meter and a dangling meter to the grid.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    let dangling_meter = builder.meter();
    builder.connect(grid_meter, meter_bat_chain);
    builder.connect(grid, dangling_meter);

    assert_eq!(meter_bat_chain.component_id(), 12);
    assert_eq!(dangling_meter.component_id(), 15);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all its suceessors
            "MAX(",
            "#1 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)) - ",
            "COALESCE(#12, #13, 0.0), ",
            "0.0) + ",
            // difference of battery meter from battery inverter and pv
            // meter from the two pv inverters.
            "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
            // difference of "mixed" meter from its successors.
            "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0) + ",
            // difference of second battery meter from inverter.
            "COALESCE(MAX(#12 - #13, 0.0), 0.0) + ",
            // consumption component of the dangling meter.
            "MAX(#15, 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of grid meter from all non-consumer meters, adding
            // the dangling meter consumption.
            "MAX(",
            "#1 + #15 - ",
            "COALESCE(#2, #3, 0.0) - ",
            "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
            "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)) - ",
            "COALESCE(#12, #13, 0.0), ",
            "0.0)",
        )
    );

    Ok(())
}

#[test]
fn test_consumer_formula_producers_directly_under_grid_meter() -> Result<(), Error> {
    // A grid meter whose only children are producer components. There is
    // no separate load meter. The grid meter carries the site's residual
    // (unmodeled) consumer load. So the producers must be subtracted by
    // their own readings. The grid meter must NOT stand in for the
    // producer group: that would subtract its whole reading and zero out
    // the residual load it is meant to report. For the same reason, the
    // grid meter is never backed by its children's sum. With the meter
    // offline, that sum holds only the producers. The formula would then
    // report a false consumer value of 0 instead of no value.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    let solar_inverter = builder.solar_inverter();
    let chp = builder.chp();
    builder.connect(grid_meter, solar_inverter);
    builder.connect(grid_meter, chp);

    assert_eq!(grid_meter.component_id(), 1);
    assert_eq!(solar_inverter.component_id(), 2);
    assert_eq!(chp.component_id(), 3);

    let graph = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        // The bare grid meter minus each producer's own reading. The grid
        // meter does NOT stand in for the producer group; that would
        // cancel to zero.
        "MAX(#1 - COALESCE(#2, 0.0) - COALESCE(#3, 0.0), 0.0)",
    );

    Ok(())
}

#[test]
fn test_consumer_formula_without_grid_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a meter-inverter-battery chain to the grid component.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid, meter_bat_chain);

    assert_eq!(meter_bat_chain.component_id(), 1);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );
    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    // Formula subtracts inverter from battery meter, or shows zero
    // consumption if either of the components have no data.
    assert_eq!(formula, "COALESCE(MAX(#1 - #2, 0.0), 0.0)");
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    // The meter is treated as a battery meter, so its consumption is 0.
    assert_eq!(formula, "0.0");

    // Add a pv meter with one solar inverter and two dangling meter.
    let meter_pv_chain = builder.meter_pv_chain(1);
    let dangling_meter_1 = builder.meter();
    let dangling_meter_2 = builder.meter();
    builder.connect(grid, meter_pv_chain);
    builder.connect(grid, dangling_meter_1);
    builder.connect(grid, dangling_meter_2);

    assert_eq!(meter_pv_chain.component_id(), 4);
    assert_eq!(dangling_meter_1.component_id(), 6);
    assert_eq!(dangling_meter_2.component_id(), 7);

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // subtract meter successors from meters
            "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
            // dangling meters
            "MAX(#6, 0.0) + MAX(#7, 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#6 + #7, 0.0)");

    // Add a battery inverter to the grid, without a battery meter.
    //
    // This shouldn't show up in the formula, because battery inverter
    // consumption is charging, not site consumption.
    let inv_bat_chain = builder.inv_bat_chain(1);
    builder.connect(grid, inv_bat_chain);

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // subtract meter successors from meters
            "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
            // dangling meters
            "MAX(#6, 0.0) + MAX(#7, 0.0)"
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#6 + #7, 0.0)");

    // Add a PV inverter and a CHP to the grid, without a meter.
    //
    // Their consumption is counted as site consumption, because they can't
    // be taken out, by discharging the batteries, for example.
    let pv_inv = builder.solar_inverter();
    let chp = builder.chp();
    builder.connect(grid, pv_inv);
    builder.connect(grid, chp);

    assert_eq!(pv_inv.component_id(), 10);
    assert_eq!(chp.component_id(), 11);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // subtract meter successors from meters
            "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
            // dangling meters
            "MAX(#6, 0.0) + MAX(#7, 0.0) + ",
            // PV inverter and CHP
            "MAX(#11, 0.0) + MAX(#10, 0.0)",
        )
    );
    let graph_no_phantom = builder.build(None)?;
    let formula = graph_no_phantom.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#6 + #7, 0.0)");

    Ok(())
}

/// In the no-grid-meter consumer formula, a consumer meter that provides no
/// telemetry has no reading and is dropped from the sum.
///
/// Topology (ids): `Grid:0 → BatMeter:1 → Inv:2 → Bat:3`, plus dangling
/// consumer meters `Grid:0 → Meter:4` and `Grid:0 → Meter:5 (no telemetry)`.
#[test]
fn test_consumer_formula_without_grid_meter_skips_no_telemetry() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    // A battery chain keeps at least one non-grid-meter successor, selecting
    // the no-grid-meter branch.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid, meter_bat_chain);
    let dangling = builder.meter();
    let dangling_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    builder.connect(grid, dangling);
    builder.connect(grid, dangling_no_telemetry);

    assert_eq!(dangling.component_id(), 4);
    assert_eq!(dangling_no_telemetry.component_id(), 5);

    let graph = builder.build(None)?;
    // Only the reporting dangling meter #4 is summed; #5 is dropped.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#4, 0.0)");
    Ok(())
}

/// In the no-grid-meter consumer formula, a no-telemetry consumer meter is
/// measured by its reporting descendant meters instead of hiding its
/// subtree.
///
/// Topology (ids): `Grid:0 → BatMeter:1 → Inv:2 → Bat:3`, plus
/// `Grid:0 → Meter:4 (no telemetry) → Meter:5`.
#[test]
fn test_consumer_formula_without_grid_meter_no_telemetry_meter_children() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    // A battery chain keeps at least one non-grid-meter successor, selecting
    // the no-grid-meter branch.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid, meter_bat_chain);
    let no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let child = builder.meter();
    builder.connect(grid, no_telemetry);
    builder.connect(no_telemetry, child);

    assert_eq!(child.component_id(), 5);

    let graph = builder.build(None)?;
    // Meter #4 has no reading; its reporting child meter #5 is summed in its
    // place.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#5, 0.0)");
    Ok(())
}

/// In the phantom-loads consumer formula, a component that provides no
/// telemetry is not subtracted from its meter (its share counts as phantom
/// load), and never contributes a reading.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, PV:3 (no telemetry)}`.
#[test]
fn test_consumer_formula_phantom_loads_skips_no_telemetry() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let sub_meter = builder.meter();
    builder.connect(grid_meter, sub_meter);
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(grid_meter, pv_no_telemetry);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // #3 is neither subtracted from meter #1 nor given a term of its own; its
    // consumption stays in meter #1's residual.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - #2, 0.0) + MAX(#2, 0.0)",
    );
    Ok(())
}

/// In the phantom-loads consumer formula, a meter that provides no telemetry
/// contributes no residual term, and a no-telemetry non-meter grid successor
/// contributes nothing.
///
/// Topology (ids): `Grid:0 → Meter:1 (no telemetry) → Meter:2`, plus
/// `Grid:0 → CHP:3 (no telemetry)`.
#[test]
fn test_consumer_formula_phantom_loads_skips_no_telemetry_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let child = builder.meter();
    builder.connect(grid, no_telemetry);
    builder.connect(no_telemetry, child);
    let chp_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Chp, OperationalMode::ControlOnly);
    builder.connect(grid, chp_no_telemetry);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // Meter #1's residual is unknowable and CHP #3 has no reading; only the
    // reporting meter #2 keeps a term.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#2, 0.0)");
    // Both omissions are deliberate, so both get silent nodes: the meter's
    // was always recorded, the CHP's must not be lost to the successor
    // filter.
    let explained = ConsumerFormulaBuilder::try_new(&graph)?.build_explained()?;
    for id in [no_telemetry.component_id(), chp_no_telemetry.component_id()] {
        assert!(
            explained.explanation.nodes().iter().any(|node| {
                node.kind == ExplanationKind::NoTelemetryZero
                    && node.component_ids == [id]
                    && node.rendered().is_none()
            }),
            "missing silent node for #{id}"
        );
    }
    Ok(())
}

/// In the phantom-loads consumer formula, a battery inverter under the grid
/// adds no term — battery flow is not site consumption — and the deliberate
/// omission is recorded as a silent node naming that reason. The storage
/// reason wins over the no-telemetry one: a silent battery inverter is
/// excluded for being storage, not for its missing reading.
///
/// Topology (ids): `Grid:0 → BatteryInverter:1 → Battery:2`, plus
/// `Grid:0 → BatteryInverter:3 (no telemetry) → Battery:4`.
#[test]
fn test_consumer_formula_phantom_loads_excludes_battery_inverter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(grid, inverter);
    builder.connect(inverter, battery);
    let silent_inverter = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let silent_battery = builder.battery();
    builder.connect(grid, silent_inverter);
    builder.connect(silent_inverter, silent_battery);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    assert_eq!(graph.consumer_formula()?.to_string(), "0.0");
    let explained = ConsumerFormulaBuilder::try_new(&graph)?.build_explained()?;
    for id in [inverter.component_id(), silent_inverter.component_id()] {
        assert!(
            explained.explanation.nodes().iter().any(|node| {
                node.kind == ExplanationKind::StorageNotConsumption
                    && node.component_ids == [id]
                    && node.rendered().is_none()
            }),
            "missing storage node for #{id}"
        );
    }
    assert!(
        !explained.explanation.nodes().iter().any(|node| {
            node.kind == ExplanationKind::NoTelemetryZero
                && node.component_ids == [silent_inverter.component_id()]
        }),
        "the storage reason must replace the no-telemetry one, not join it"
    );
    Ok(())
}

/// In the phantom-loads consumer formula, a diamond sibling meter that
/// provides no telemetry is left out of the diamond sum while the shared
/// successors are still subtracted.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2 (no telemetry)} → Meter:3`.
#[test]
fn test_consumer_formula_phantom_loads_no_telemetry_diamond_sibling() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let meter = builder.meter();
    let sibling_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let shared = builder.meter();
    builder.connect(grid, meter);
    builder.connect(grid, sibling_no_telemetry);
    builder.connect(meter, shared);
    builder.connect(sibling_no_telemetry, shared);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // Sibling #2's reading is left out of the diamond sum; the shared meter
    // #3 is still subtracted and keeps its own residual term.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - #3, 0.0) + MAX(#3, 0.0)",
    );
    Ok(())
}

/// A partial diamond closes transitively: Meter:2 bridges Meter:1 and
/// Meter:3 by sharing one child with each, so all three form one group.
/// Splitting them would add the bridge's reading to two groups and
/// subtract each shared child twice.
///
/// Topology (ids): `Grid:0 → {Meter:1 → Meter:4, Meter:2 → {Meter:4,
/// Meter:5}, Meter:3 → Meter:5}`.
#[test]
fn test_consumer_formula_phantom_loads_merges_a_bridged_diamond() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let left = builder.meter();
    let bridge = builder.meter();
    let right = builder.meter();
    let shared_left = builder.meter();
    let shared_right = builder.meter();
    builder.connect(grid, left);
    builder.connect(grid, bridge);
    builder.connect(grid, right);
    builder.connect(left, shared_left);
    builder.connect(bridge, shared_left);
    builder.connect(bridge, shared_right);
    builder.connect(right, shared_right);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #2 + #3 - #4 - #5, 0.0) + MAX(#4, 0.0) + MAX(#5, 0.0)",
    );
    Ok(())
}

/// The bridged diamond with the bridge silent: its reading is left out
/// of the group sum, but the group still merges, so each shared child is
/// subtracted once — not once per line it feeds.
///
/// Topology (ids): as the bridged-diamond test, Meter:2 without
/// telemetry.
#[test]
fn test_consumer_formula_phantom_loads_merges_a_silent_bridged_diamond() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let left = builder.meter();
    let bridge =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let right = builder.meter();
    let shared_left = builder.meter();
    let shared_right = builder.meter();
    builder.connect(grid, left);
    builder.connect(grid, bridge);
    builder.connect(grid, right);
    builder.connect(left, shared_left);
    builder.connect(bridge, shared_left);
    builder.connect(bridge, shared_right);
    builder.connect(right, shared_right);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #3 - #4 - #5, 0.0) + MAX(#4, 0.0) + MAX(#5, 0.0)",
    );
    Ok(())
}

/// A reporting meter whose only child is a no-telemetry meter resolves
/// through it: the grandchild reading backs the meter's own reading in
/// the subtracted term, instead of a 0.0 that would hide it.
///
/// Topology (ids): `Grid:0 → GridMeter:1 → {Meter:2 → Meter:3 (no
/// telemetry) → Meter:4, Meter:5}`.
#[test]
fn test_consumer_formula_recurses_through_no_telemetry_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let meter = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let descendant = builder.meter();
    let load = builder.meter();
    builder.connect(grid_meter, meter);
    builder.connect(meter, silent);
    builder.connect(silent, descendant);
    builder.connect(grid_meter, load);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    // Line 2 is subtracted as COALESCE(#2, #4): the silent meter #3's
    // reporting child #4 backs #2's reading.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2, #4) - #5, 0.0) + MAX(#2 - #4, 0.0) + MAX(#4, 0.0) + MAX(#5, 0.0)"
    );
    Ok(())
}

/// A no-telemetry consumer meter on a parallel feed is descended past,
/// but a descendant meter that another collected meter already measures
/// is not summed next to it: that line would be counted twice.
///
/// Topology (ids): `Grid:0 → {BatMeter:1 → Inv:2 → Bat:3, Meter:4 (no
/// telemetry), Meter:5}`, with `4 → 6` and `5 → 6`.
#[test]
fn test_consumer_formula_no_telemetry_shared_descendant() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let bat_meter = builder.meter_bat_chain(1, 1);
    builder.connect(grid, bat_meter);
    let sibling_no_telemetry =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let sibling = builder.meter();
    let shared = builder.meter();
    builder.connect(grid, sibling_no_telemetry);
    builder.connect(grid, sibling);
    builder.connect(sibling_no_telemetry, shared);
    builder.connect(sibling, shared);

    let graph = builder.build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#5, 0.0)");
    Ok(())
}

#[test]
fn test_consumer_formula_diamond_meters() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add three meters to the grid
    let grid_meter_1 = builder.meter();
    let grid_meter_2 = builder.meter();
    let grid_meter_3 = builder.meter();
    builder.connect(grid, grid_meter_1);
    builder.connect(grid, grid_meter_2);
    builder.connect(grid, grid_meter_3);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );

    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(formula, "MAX(#1, 0.0) + MAX(#2, 0.0) + MAX(#3, 0.0)");

    // Add two solar inverters with two grid meters as predecessors.
    let meter_pv_chain_1 = builder.meter_pv_chain(1);
    let meter_pv_chain_2 = builder.meter_pv_chain(1);
    builder.connect(grid_meter_1, meter_pv_chain_1);
    builder.connect(grid_meter_1, meter_pv_chain_2);
    builder.connect(grid_meter_2, meter_pv_chain_1);
    builder.connect(grid_meter_2, meter_pv_chain_2);

    assert_eq!(meter_pv_chain_1.component_id(), 4);
    assert_eq!(meter_pv_chain_2.component_id(), 6);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of pv powers from first two grid meters
            "MAX(#1 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0), 0.0) + ",
            // third grid meter still dangling
            "MAX(#3, 0.0) + ",
            // difference of solar inverters from their meters
            "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0)"
        )
    );

    // Add a meter to grid meter 3, and then add the two solar inverters to
    // that meter.
    let meter = builder.meter();
    builder.connect(grid_meter_3, meter);
    builder.connect(meter, meter_pv_chain_1);
    builder.connect(meter, meter_pv_chain_2);

    assert_eq!(meter.component_id(), 8);

    let config = Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    );
    let graph = builder.build(config.clone())?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of pv powers from first two grid meters and meter#8
            "MAX(#1 + #8 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0), 0.0) + ",
            // difference of meter#8 from third grid meter
            "MAX(#3 - #8, 0.0) + ",
            // difference of solar inverters from their meters
            "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0)"
        )
    );

    // Add a battery inverter to the first grid meter.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid_meter_1, meter_bat_chain);

    let graph = builder.build(config)?;
    let formula = graph.consumer_formula()?.to_string();
    assert_eq!(
        formula,
        concat!(
            // difference of pv and battery powers from first two grid
            // meters and meter#8
            "MAX(",
            "#1 + #8 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0) - COALESCE(#9, #10, 0.0), ",
            "0.0) + ",
            // difference of meter#8 from third grid meter
            "MAX(#3 - #8, 0.0) + ",
            // difference of solar inverters from their meters
            "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0) + ",
            // difference of battery inverter from battery meter
            "COALESCE(MAX(#9 - #10, 0.0), 0.0)"
        )
    );

    Ok(())
}

#[test]
fn test_consumer_formula_mixed_meter_with_component_submeter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    // A "mixed" meter feeds a PV inverter and a battery sub-meter (itself
    // a component chain). The battery sub-meter's id sorts before the PV
    // inverter's id. So the sub-meter is first resolved onto its own
    // measurement point. Only after that does the sibling PV inverter
    // reveal that the mixed meter covers the whole group.
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let bat_submeter = builder.meter_bat_chain(1, 1);
    builder.connect(mixed_meter, bat_submeter);
    let solar_inverter = builder.solar_inverter();
    builder.connect(mixed_meter, solar_inverter);

    // A second grid-meter child so the mixed meter is not its sole child
    // (which would make the mixed meter a fallback grid meter).
    let pv_meter = builder.meter_pv_chain(1);
    builder.connect(grid_meter, pv_meter);

    assert_eq!(grid.component_id(), 0);
    assert_eq!(grid_meter.component_id(), 1);
    assert_eq!(mixed_meter.component_id(), 2);
    assert_eq!(bat_submeter.component_id(), 3);
    assert_eq!(solar_inverter.component_id(), 6);
    assert_eq!(pv_meter.component_id(), 7);

    let graph = builder.build(None)?;
    let formula = graph.consumer_formula()?.to_string();
    // The mixed meter (#2) covers its whole group. So it is subtracted
    // once, and the battery sub-meter (#3) is NOT subtracted again on its
    // own. The children of #2 back its reading. This is recursive: the
    // sub-meter's own children (#4) back the sub-meter. So an offline
    // meter does not make the whole formula null while the leaf
    // components still report.
    assert_eq!(
        formula,
        concat!(
            "MAX(#1 - COALESCE(#2, COALESCE(#6, 0.0) + COALESCE(#3, #4, 0.0)) - ",
            "COALESCE(#7, #8, 0.0), 0.0)"
        )
    );

    Ok(())
}

/// The same overlap under a grid meter. Meter:3 and Meter:4 both feed
/// BatteryInverter:6, so neither reads the whole chain; the pair minus
/// Meter:3's other child does.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2, Meter:3}`,
/// `Meter:2 → {Meter:4, Meter:5}`, `Meter:4 → BatteryInverter:6 →
/// Battery:7`, `Meter:3 → {BatteryInverter:6, Meter:8}`.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_an_outside_fed_chain_once() -> Result<(), Error>
{
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let feeder = builder.meter();
    let sibling = builder.meter();
    let battery_meter = builder.meter();
    let load = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let sibling_load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, feeder);
    builder.connect(grid_meter, sibling);
    builder.connect(feeder, battery_meter);
    builder.connect(feeder, load);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(sibling, inverter);
    builder.connect(sibling, sibling_load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#3 + #4 - #8, #6, 0.0), 0.0)"
    );

    Ok(())
}

/// The chains under a replaced meter all become targets, not just the one
/// the overlap was found through. Meter:2 also measures BatteryInverter:6,
/// which has no second feed and would otherwise go unsubtracted.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2, Meter:3}`,
/// `Meter:2 → {BatteryInverter:4 → Battery:5, BatteryInverter:6 →
/// Battery:7}`, `Meter:3 → {BatteryInverter:4, Meter:8}`.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_every_chain_under_a_replaced_meter()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let battery_meter = builder.meter();
    let sibling = builder.meter();
    let shared = builder.battery_inverter();
    let shared_battery = builder.battery();
    let own = builder.battery_inverter();
    let own_battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(grid_meter, sibling);
    builder.connect(battery_meter, shared);
    builder.connect(shared, shared_battery);
    builder.connect(battery_meter, own);
    builder.connect(own, own_battery);
    builder.connect(sibling, shared);
    builder.connect(sibling, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2 + #3 - #8, COALESCE(#4, 0.0) + COALESCE(#6, 0.0)), 0.0)"
    );

    Ok(())
}

/// Two independent overlaps are both resolved, not just the first.
///
/// Topology (ids): `Grid:0 → Meter:1`,
/// `Meter:1 → {Meter:2, Meter:5, Meter:7, Meter:10}`,
/// `Meter:2 → BatteryInverter:3 → Battery:4`, `Meter:5 → {BatteryInverter:3,
/// Meter:6}`, `Meter:7 → BatteryInverter:8 → Battery:9`,
/// `Meter:10 → {BatteryInverter:8, Meter:11}`.
#[test]
fn test_consumer_formula_with_grid_meter_resolves_every_overlap() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    for _ in 0..2 {
        let battery_meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        let sibling = builder.meter();
        let load = builder.meter();
        builder.connect(grid_meter, battery_meter);
        builder.connect(battery_meter, inverter);
        builder.connect(inverter, battery);
        builder.connect(grid_meter, sibling);
        builder.connect(sibling, inverter);
        builder.connect(sibling, load);
    }

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2 + #5 - #6, #3, 0.0) - COALESCE(#7 + #10 - #11, #8, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain that reports nothing cannot replace the meter above it: the
/// meter's reading is the only measurement left, so it stays the target.
/// BatteryInverter:3 provides no telemetry, and no feed can stand in for
/// it — Meter:1 is the grid meter — so its own term would be a plain 0.0.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2,
/// BatteryInverter:3, Meter:5}`, `Meter:2 → BatteryInverter:3 → Battery:4`.
#[test]
fn test_consumer_formula_with_grid_meter_keeps_the_meter_of_a_silent_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid_meter, inverter);
    builder.connect(grid_meter, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#2, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain that is silent from its meter down cannot be subtracted at
/// all: no reading can be built from any of it, so no term is emitted.
/// Battery:5 still reports, but no term reaches its reading through the
/// two silent components above it.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:1 → {Meter:2,
/// BatteryInverter:4, Meter:6}`, `Meter:2 → Meter:3 (no telemetry) →
/// BatteryInverter:4 (no telemetry) → Battery:5`.
#[test]
fn test_consumer_formula_with_grid_meter_emits_no_term_for_a_wholly_silent_chain()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let battery_meter = builder.meter();
    let silent_meter =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let inverter = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(battery_meter, silent_meter);
    builder.connect(silent_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid_meter, inverter);
    builder.connect(grid_meter, load);

    let graph = builder.build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#1, 0.0)");

    Ok(())
}

/// A chain fed through two parallel meters, one silent: the silent
/// meter's term would be a plain 0.0, and the reporting meter reads only
/// its own feed. The chain's own reading covers both feeds, so it is the
/// term — the same replacement the wholly-silent case makes, in sibling
/// form.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3 (no
/// telemetry)}`, both → `PV:4`.
#[test]
fn test_consumer_formula_with_grid_meter_subtracts_a_silently_fed_chain_by_its_reading()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let pv_meter = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let pv = builder.solar_inverter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, pv_meter);
    builder.connect(grid_meter, silent);
    builder.connect(pv_meter, pv);
    builder.connect(silent, pv);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#4, 0.0), 0.0)"
    );

    Ok(())
}

/// A silent parallel meter that also feeds a chain of its own is still
/// replaced: its reading covers nothing, so at best it reaches the
/// chains only it feeds. Both chains come out through their own
/// readings; the shared one covers both feeds.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3 (no
/// telemetry)}`, `Meter:2 → PV:4`, `Meter:3 → {PV:4, PV:5}`.
#[test]
fn test_consumer_formula_with_grid_meter_replaces_a_silent_meter_that_still_reads_a_chain()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let pv_meter = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let shared_pv = builder.solar_inverter();
    let own_pv = builder.solar_inverter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, pv_meter);
    builder.connect(grid_meter, silent);
    builder.connect(pv_meter, shared_pv);
    builder.connect(silent, shared_pv);
    builder.connect(silent, own_pv);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 - COALESCE(#4, 0.0) - COALESCE(#5, 0.0), 0.0)"
    );

    Ok(())
}

/// Without a grid meter, a feeder meter's reading covers the whole
/// feeder, so a battery chain under it is subtracted back out. The
/// formula matches the shape the grid-meter path produces.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3 → BatteryInverter:4 → Battery:5, Meter:6}`.
/// PV:1 is unmetered, which is what selects the no-grid-meter path.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_battery_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let feeder = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, feeder);
    builder.connect(feeder, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#2 - COALESCE(#3, #4, 0.0), 0.0)"
    );

    // Without fallbacks the chain is subtracted through its meter alone.
    let no_fallback = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(
        no_fallback.consumer_formula()?.to_string(),
        "MAX(#2 - #3, 0.0)"
    );

    Ok(())
}

/// Every feeder meter is summed and every chain under one is
/// subtracted, each through its own meter.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:5, PV:9}`,
/// `Meter:1 → {Meter:2 → BatteryInverter:3 → Battery:4}`,
/// `Meter:5 → {Meter:6 → PV:7, Meter:8}`.
/// PV:9 is unmetered, which is what selects the no-grid-meter path:
/// without it both feeder meters are grid meters and the graph takes
/// the grid-meter path instead.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_chain_per_feeder() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let feeder_a = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let feeder_b = builder.meter();
    let pv_meter = builder.meter();
    let pv = builder.solar_inverter();
    let load = builder.meter();

    builder.connect(grid, feeder_a);
    builder.connect(feeder_a, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid, feeder_b);
    builder.connect(feeder_b, pv_meter);
    builder.connect(pv_meter, pv);
    builder.connect(feeder_b, load);
    let stray = builder.solar_inverter();
    builder.connect(grid, stray);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #5 - COALESCE(#2, #3, 0.0) - COALESCE(#6, #7, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain that hangs off the grid rather than under a feeder meter is
/// never part of the sum, so it must not be subtracted either. It is
/// fed by the grid, not by a kept meter, so `reached_only_through`
/// rejects it.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2 → Meter:3}`.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_chain_outside_the_sum() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let feeder = builder.meter();
    let load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, feeder);
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    // #1 is a PV chain, but it sits outside every feeder meter.
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#2, 0.0)");

    Ok(())
}

/// A chain fed from outside the summed meters stays counted. Only part
/// of BatteryInverter:2's throughput passes through Meter:1, so
/// subtracting its whole reading would take out power the sum never
/// added.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:4}`,
/// `Meter:1 → {BatteryInverter:2 → Battery:3, Meter:5}`,
/// `Meter:4 → BatteryInverter:2`.
/// Meter:4 is a battery meter, so it is not a grid meter and the
/// no-grid-meter path runs.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_chain_with_an_outside_feed() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let feeder = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let battery_meter = builder.meter();
    let load = builder.meter();

    builder.connect(grid, feeder);
    builder.connect(feeder, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#1, 0.0)");

    Ok(())
}

/// A chain fed by two summed meters is subtracted once, through its own
/// meter. The chain is found once however many kept meters feed it, so
/// it gets one term and not one per feeder.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2, Meter:3}`,
/// `Meter:2 → Meter:4`, `Meter:3 → Meter:4`,
/// `Meter:4 → BatteryInverter:5 → Battery:6`.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_shared_chain_once() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let feeder_a = builder.meter();
    let feeder_b = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();

    builder.connect(grid, pv);
    builder.connect(grid, feeder_a);
    builder.connect(grid, feeder_b);
    builder.connect(feeder_a, battery_meter);
    builder.connect(feeder_b, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#2 + #3 - COALESCE(#4, #5, 0.0), 0.0)"
    );

    Ok(())
}

/// Every chain kind is subtracted: battery, PV, CHP, EV charger, wind
/// turbine and steam boiler.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`, and under Meter:2 one
/// chain of each kind — `Meter:3 → BatteryInverter:4 → Battery:5`,
/// `Meter:6 → PV:7`, `Meter:8 → CHP:9`, `Meter:10 → EvCharger:11`,
/// `Meter:12 → WindTurbine:13`, `Meter:14 → SteamBoiler:15`.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_every_chain_kind() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv_at_grid = builder.solar_inverter();
    let feeder = builder.meter();

    builder.connect(grid, pv_at_grid);
    builder.connect(grid, feeder);
    for chain in [
        builder.meter_bat_chain(1, 1),
        builder.meter_pv_chain(1),
        builder.meter_chp_chain(1),
        builder.meter_ev_charger_chain(1),
        builder.meter_wind_turbine_chain(1),
        builder.meter_steam_boiler_chain(1),
    ] {
        builder.connect(feeder, chain);
    }

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#2 - COALESCE(#3, #4, 0.0) - COALESCE(#6, #7, 0.0) \
         - COALESCE(#8, #9, 0.0) - COALESCE(#10, #11, 0.0) \
         - COALESCE(#12, #13, 0.0) - COALESCE(#14, #15, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain component with no meter of its own is subtracted by its own
/// reading — the "inverter wired straight to a feeder" shape.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {BatteryInverter:3 → Battery:4, Meter:5}`.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_an_unmetered_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let feeder = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, feeder);
    builder.connect(feeder, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#2 - COALESCE(#3, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain under a covered meter stays counted when that meter also has
/// a feed the sum cannot see. Meter:4 is covered by Meter:2, so only
/// Meter:2 is summed, and part of the battery's power reaches Meter:4
/// through the silent Meter:3. Subtracting the chain would remove power
/// the sum never added, so the check runs against the kept meters, not
/// against every meter discovery found.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2, Meter:3 (no telemetry)}`,
/// `Meter:2 → Meter:4`, `Meter:3 → Meter:4`,
/// `Meter:4 → Meter:5 → BatteryInverter:6 → Battery:7`.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_silently_fed_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let feeder = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let covered = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();

    builder.connect(grid, pv);
    builder.connect(grid, feeder);
    builder.connect(grid, silent);
    builder.connect(feeder, covered);
    builder.connect(silent, covered);
    builder.connect(covered, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);

    let graph = builder.build(None)?;
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#2, 0.0)");

    Ok(())
}

/// Two chains under one mixed meter are subtracted through that meter
/// once. Meter:3 measures a battery chain and a PV inverter, so it is
/// not a chain itself and each child becomes a target. The single
/// `aggregate_terms` call resolves the pair onto Meter:3; one call per
/// target would make each subtract the other and count Meter:3 twice.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:8}`,
/// `Meter:3 → {Meter:4 → BatteryInverter:5 → Battery:6, PV:7}`.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_mixed_meter_once() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let feeder = builder.meter();
    let mixed = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, feeder);
    builder.connect(feeder, mixed);
    let battery_meter = builder.meter_bat_chain(1, 1);
    builder.connect(mixed, battery_meter);
    let mixed_pv = builder.solar_inverter();
    builder.connect(mixed, mixed_pv);
    let load = builder.meter();
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#2 - COALESCE(#3, COALESCE(#7, 0.0) + COALESCE(#4, #5, 0.0)), 0.0)"
    );

    Ok(())
}

/// A chain measured through a summed meter is subtracted by its own
/// reading. Meter:3 has a load of its own, so BatteryInverter:5 would
/// resolve to `#3 - #7`; subtracting that would cancel Meter:3 out of the
/// sum and drop its whole line, so the inverter's own reading stands in.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:4}`,
/// `Meter:3 → {BatteryInverter:5 → Battery:6, Meter:7}`.
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4,
/// which are not grid meters.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_meter_measured_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let feeder = builder.meter();
    let sibling = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, silent);
    builder.connect(silent, feeder);
    builder.connect(silent, sibling);
    builder.connect(feeder, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - COALESCE(#5, 0.0), 0.0)"
    );

    // Without fallbacks the term is the inverter's own reading to begin
    // with, so there is nothing to stand in for.
    let no_fallback = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(
        no_fallback.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - #5, 0.0)"
    );

    Ok(())
}

/// A cancelling chain and a plain one are both subtracted, each from the
/// source that suits it. The battery chain under Meter:3 would resolve to
/// `#3 - #7`, so it falls back to its own reading; the PV chain under
/// Meter:4 has its own meter and keeps it.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:4}`,
/// `Meter:3 → {BatteryInverter:5 → Battery:6, Meter:7}`,
/// `Meter:4 → Meter:8 → PV:9`.
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_cancelling_chain_and_a_plain_one()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let feeder = builder.meter();
    let sibling = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();
    let pv_meter = builder.meter();
    let metered_pv = builder.solar_inverter();

    builder.connect(grid, pv);
    builder.connect(grid, silent);
    builder.connect(silent, feeder);
    builder.connect(silent, sibling);
    builder.connect(feeder, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, load);
    builder.connect(sibling, pv_meter);
    builder.connect(pv_meter, metered_pv);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - COALESCE(#5, 0.0) - COALESCE(#8, #9, 0.0), 0.0)"
    );

    Ok(())
}

/// A summed meter that measures nothing but chains is not substituted for
/// them. Meter:3 covers exactly BatteryInverter:5 and PV:7, so it would
/// normally stand in for both; using it would cancel Meter:3 out of the sum
/// and drop the loads it reads that the graph does not model. Each chain is
/// subtracted by its own reading instead.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:4}`,
/// `Meter:3 → {BatteryInverter:5 → Battery:6, PV:7}`.
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_covered_group_by_its_own_readings()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let feeder = builder.meter();
    let sibling = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let mixed_pv = builder.solar_inverter();

    builder.connect(grid, pv);
    builder.connect(grid, silent);
    builder.connect(silent, feeder);
    builder.connect(silent, sibling);
    builder.connect(feeder, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, mixed_pv);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - COALESCE(#5, 0.0) - COALESCE(#7, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain component that reports nothing cannot be subtracted. Its meter is
/// the only thing that reads it, and that meter is in the sum, so its power
/// stays counted; the reporting chain next to it still comes out.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:4}`,
/// `Meter:3 → {BatteryInverter:5 (no telemetry) → Battery:6, PV:7, Meter:8}`.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_a_chain_that_reports_nothing() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let feeder = builder.meter();
    let sibling = builder.meter();
    let inverter = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let battery = builder.battery();
    let mixed_pv = builder.solar_inverter();
    let load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, silent);
    builder.connect(silent, feeder);
    builder.connect(silent, sibling);
    builder.connect(feeder, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, mixed_pv);
    builder.connect(feeder, load);

    let graph = builder.build(None)?;
    // BatteryInverter:5 contributes a 0.0 term: there is no reading to take
    // out, so its draw stays in Meter:3's reading.
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - 0.0 - COALESCE(#7, 0.0), 0.0)"
    );

    Ok(())
}

/// One off-limits parent meter is enough to refuse the group. PV:7 is fed by
/// Meter:5, which is not summed, and by Meter:4, which is; measuring it as
/// that pair minus their other children would cancel Meter:4 out of the sum.
/// Its own reading is used instead.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:4}`, `Meter:3 → {Meter:5, Meter:6}`,
/// `Meter:5 → {PV:7, Meter:8}`, `Meter:4 → {PV:7, Meter:9}`.
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_refuses_a_partly_off_limits_diamond() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let feeder = builder.meter();
    let sibling = builder.meter();
    let nested = builder.meter();
    let nested_load = builder.meter();
    let shared_pv = builder.solar_inverter();
    let load = builder.meter();
    let sibling_load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, silent);
    builder.connect(silent, feeder);
    builder.connect(silent, sibling);
    builder.connect(feeder, nested);
    builder.connect(feeder, nested_load);
    builder.connect(nested, shared_pv);
    builder.connect(nested, load);
    builder.connect(sibling, shared_pv);
    builder.connect(sibling, sibling_load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - COALESCE(#7, 0.0), 0.0)"
    );

    Ok(())
}

/// A chain fed both through its own meter and from elsewhere is subtracted
/// once, by its own reading. Meter:5 only reads what reaches
/// BatteryInverter:7 through it, so its reading is not the whole chain's;
/// subtracting both would take that part out twice.
///
/// Topology (ids): `Grid:0 → {PV:1, Meter:2}`,
/// `Meter:2 → {Meter:3, Meter:4}`, `Meter:3 → {Meter:5, Meter:6}`,
/// `Meter:5 → BatteryInverter:7 → Battery:8`, `Meter:4 → {BatteryInverter:7,
/// Meter:9}`.
/// Meter:2 reports nothing, so the sum descends to Meter:3 and Meter:4.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_an_outside_fed_chain_once() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv = builder.solar_inverter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let feeder = builder.meter();
    let sibling = builder.meter();
    let battery_meter = builder.meter();
    let load = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let sibling_load = builder.meter();

    builder.connect(grid, pv);
    builder.connect(grid, silent);
    builder.connect(silent, feeder);
    builder.connect(silent, sibling);
    builder.connect(feeder, battery_meter);
    builder.connect(feeder, load);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(sibling, inverter);
    builder.connect(sibling, sibling_load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#3 + #4 - COALESCE(#7, 0.0), 0.0)"
    );

    Ok(())
}

/// When the sum does not hold the whole chain, the meter above it is the
/// right term after all: it reads exactly the part the sum does hold.
/// BatteryInverter:3 is also fed by the silent Meter:6, whose power the sum
/// never added, so Meter:2 — not the inverter — is subtracted.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:6 (no telemetry), PV:8}`,
/// `Meter:1 → {Meter:2, Meter:5}`, `Meter:2 → BatteryInverter:3 → Battery:4`,
/// `Meter:6 → {Meter:7, BatteryInverter:3}`.
#[test]
fn test_consumer_formula_no_grid_meter_subtracts_a_partly_held_chain_through_its_meter()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let feeder = builder.meter();
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let silent_load = builder.meter();
    let pv = builder.solar_inverter();

    builder.connect(grid, feeder);
    builder.connect(feeder, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(feeder, load);
    builder.connect(grid, silent);
    builder.connect(silent, silent_load);
    builder.connect(silent, inverter);
    builder.connect(grid, pv);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #7 - #2, 0.0)"
    );

    Ok(())
}

/// A replacement chain the sum does not hold is dropped, not subtracted.
/// Meter:4 is replaced — BatteryInverter:5 below it is also fed by Meter:2
/// and must come out through its own reading — but its other chain,
/// BatteryInverter:7, is fed by the silent Meter:3 straight off the grid,
/// so the sum never held it in full and it stays counted.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2, Meter:3 (no telemetry)}`,
/// `Meter:1 → {Meter:4, Meter:9}`, `Meter:4 → {BatteryInverter:5 →
/// Battery:6, BatteryInverter:7 → Battery:8}`, `Meter:2 →
/// {BatteryInverter:5, Meter:10}`, `Meter:3 → BatteryInverter:7`.
#[test]
fn test_consumer_formula_no_grid_meter_keeps_an_uncovered_chain_under_a_replaced_meter()
-> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let feeder = builder.meter();
    let sibling = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let battery_meter = builder.meter();
    let shared = builder.battery_inverter();
    let shared_battery = builder.battery();
    let outside_fed = builder.battery_inverter();
    let outside_fed_battery = builder.battery();
    let load = builder.meter();
    let sibling_load = builder.meter();

    builder.connect(grid, feeder);
    builder.connect(grid, sibling);
    builder.connect(grid, silent);
    builder.connect(feeder, battery_meter);
    builder.connect(feeder, load);
    builder.connect(battery_meter, shared);
    builder.connect(shared, shared_battery);
    builder.connect(battery_meter, outside_fed);
    builder.connect(outside_fed, outside_fed_battery);
    builder.connect(sibling, shared);
    builder.connect(sibling, sibling_load);
    builder.connect(silent, outside_fed);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.consumer_formula()?.to_string(),
        "MAX(#1 + #2 - COALESCE(#5, 0.0), 0.0)"
    );

    Ok(())
}

/// A grid meter that reports nothing leaves no reading to subtract from,
/// so the graph takes the summed-meters shape: the topmost reporting
/// meters below the silent grid meter measure the site. The battery
/// chain is not covered by the sum and stays out; the grid formula
/// descends to the same readings.
///
/// Topology (ids): `Grid:0 → Meter:1 (no telemetry)`,
/// `Meter:1 → {Meter:2 → BatteryInverter:3 → Battery:4, Meter:5}`.
#[test]
fn test_consumer_formula_sums_reporting_meters_under_a_silent_grid_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(grid_meter, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.grid_formula()?.to_string(),
        "#5 + COALESCE(#2, #3, 0.0)"
    );
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#5, 0.0)");

    Ok(())
}

/// A silent grid meter next to a reporting one: the graph takes the
/// summed-meters shape, and the silent meter's line is measured by its
/// reporting children. The load behind it counts; the battery chain is
/// not covered by the sum and must not come out of it — subtracting
/// `COALESCE(#3, #4, 0.0)` would report a discharging battery as site
/// consumption.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2 (no telemetry)}`,
/// `Meter:2 → {Meter:3 → BatteryInverter:4 → Battery:5, Meter:6}`.
#[test]
fn test_consumer_formula_descends_past_a_silent_grid_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    let battery_meter = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    let load = builder.meter();

    builder.connect(grid, grid_meter);
    builder.connect(grid, silent);
    builder.connect(silent, battery_meter);
    builder.connect(battery_meter, inverter);
    builder.connect(inverter, battery);
    builder.connect(silent, load);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.grid_formula()?.to_string(),
        "#1 + #6 + COALESCE(#3, #4, 0.0)"
    );
    assert_eq!(graph.consumer_formula()?.to_string(), "MAX(#1 + #6, 0.0)");

    Ok(())
}

/// An AC path that is silent to the leaves has no reading, even when a
/// DC-side battery reports: no emitted term could use that reading, so
/// both formulas answer `None` instead of a fabricated 0.0.
///
/// Topology (ids): `Grid:0 → Meter:1 (no telemetry) → Meter:2 (no
/// telemetry) → BatteryInverter:3 (no telemetry) → Battery:4`.
#[test]
fn test_consumer_formula_is_none_when_the_ac_path_is_silent() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let gm = builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let bm = builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    let inv = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Battery),
        OperationalMode::ControlOnly,
    );
    let bat = builder.battery();
    builder.connect(grid, gm);
    builder.connect(gm, bm);
    builder.connect(bm, inv);
    builder.connect(inv, bat);

    let graph = builder.build(None)?;
    assert_eq!(graph.grid_formula()?.to_string(), "None");
    assert_eq!(graph.consumer_formula()?.to_string(), "None");

    Ok(())
}

/// A silent grid meter with nothing below it gives the graph no reading
/// at all: the consumer answers `None` like the grid formula, not a
/// fabricated zero. A genuinely meterless graph still answers 0.0.
///
/// Topology (ids): `Grid:0 → Meter:1 (no telemetry)`.
#[test]
fn test_consumer_formula_is_none_when_nothing_reports() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let gm = builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    builder.connect(grid, gm);

    let graph = builder.build(None)?;
    assert_eq!(graph.grid_formula()?.to_string(), "None");
    assert_eq!(graph.consumer_formula()?.to_string(), "None");

    Ok(())
}

/// A no-telemetry diamond sibling is recorded as a silent part also when its
/// id sorts after the reporting sibling's (the reporting sibling then visits
/// the diamond first and removes it before the main loop reaches it).
#[test]
fn test_no_telemetry_diamond_sibling_gets_silent_node() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let meter_a = builder.meter();
    builder.connect(grid, meter_a);
    let silent_meter =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::ControlOnly);
    builder.connect(grid, silent_meter);
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(meter_a, inverter);
    builder.connect(silent_meter, inverter);
    builder.connect(inverter, battery);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .include_phantom_loads_in_consumer_formula(true)
            .build(),
    ))?;
    let explained = ConsumerFormulaBuilder::try_new(&graph)?.build_explained()?;
    assert!(explained.explanation.nodes().iter().any(|node| {
        node.kind == ExplanationKind::NoTelemetryZero
            && node.component_ids == [silent_meter.component_id()]
            && node.rendered().is_none()
            && node
                .rationale
                .starts_with("Diamond sibling battery meter #2")
    }));
    Ok(())
}
