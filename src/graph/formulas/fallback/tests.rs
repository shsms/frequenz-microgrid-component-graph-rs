// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Tests for the fallback formula engine.

use std::collections::BTreeSet;

use super::super::explain::{Explanation, ExplanationKind};
use super::{SourcePreference, aggregate};
use crate::graph::test_utils::ComponentGraphBuilder;
use crate::{ComponentCategory, ComponentGraphConfig, Error, InverterType, OperationalMode};

/// The first node (pre-order) whose kind satisfies `pred`, if any.
fn find_kind<'a>(
    node: &'a Explanation,
    pred: &impl Fn(&ExplanationKind) -> bool,
) -> Option<&'a Explanation> {
    node.nodes().into_iter().find(|node| pred(&node.kind))
}

/// Test fallback expression generation when there are no meters in the
/// graph, with only PV inverters directly connected to the grid.
#[test]
fn test_no_meters() {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    let inverter = builder.solar_inverter();
    builder.connect(grid, inverter);

    let graph = builder.build(None).unwrap();
    let expr = graph.pv_formula(None).unwrap().to_string();
    assert_eq!(expr, "COALESCE(#1, 0.0)");

    let inverter = builder.solar_inverter();
    builder.connect(grid, inverter);

    let graph = builder.build(None).unwrap();
    let expr = graph.pv_formula(None).unwrap().to_string();
    assert_eq!(expr, "COALESCE(#1, 0.0) + COALESCE(#2, 0.0)");
}

/// Aggregation over meters and components: meter substitution for component
/// groups, the meter-vs-component source preference, meter chains, and the
/// raw-sum shortcut under `disable_fallback_components`.
#[test]
fn test_aggregate() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    // Add a grid meter.
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);

    // Add a battery meter with one inverter and one battery.
    let meter_bat_chain = builder.meter_bat_chain(1, 1);
    builder.connect(grid_meter, meter_bat_chain);

    assert_eq!(grid_meter.component_id(), 1);
    assert_eq!(meter_bat_chain.component_id(), 2);

    let graph = builder.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([1]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "#1");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "#1 + COALESCE(#3, #2, 0.0)");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

    let expr = aggregate(
        &graph,
        BTreeSet::from([3]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
    let expr = aggregate(
        &graph,
        BTreeSet::from([3]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
    let expr = aggregate(
        &graph,
        BTreeSet::from([2]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "#1 + #2");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "#1 + #2");

    let expr = aggregate(
        &graph,
        BTreeSet::from([3]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "#3");

    // Add a battery meter with three inverter and three batteries
    let meter_bat_chain = builder.meter_bat_chain(3, 3);
    builder.connect(grid_meter, meter_bat_chain);

    assert_eq!(meter_bat_chain.component_id(), 5);

    let graph = builder.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([3, 5]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#3, #2, 0.0) + ",
            "COALESCE(",
            "#8 + #7 + #6, ",
            "#5, ",
            "COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)",
            ")"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 5]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#2, #3, 0.0) + ",
            "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 6, 7, 8]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#2, #3, 0.0) + ",
            "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 7, 8]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#2, #3, 0.0) + ",
            "COALESCE(#5 - #6, COALESCE(#7, 0.0) + COALESCE(#8, 0.0))"
        )
    );

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([3, 5]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "#3 + #5");

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 5]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "#2 + #5");

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 6, 7, 8]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "#2 + #6 + #7 + #8");

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 7, 8]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "#2 + #7 + #8");

    let meter = builder.meter();
    let chp = builder.chp();
    let pv_inverter = builder.solar_inverter();
    builder.connect(grid_meter, meter);
    builder.connect(meter, chp);
    builder.connect(meter, pv_inverter);

    assert_eq!(meter.component_id(), 12);
    assert_eq!(chp.component_id(), 13);
    assert_eq!(pv_inverter.component_id(), 14);

    let graph = builder.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([5, 12]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) + ",
            "COALESCE(#12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0))"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([7, 14]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        "COALESCE(#7, #5 - #6 - #8, 0.0) + COALESCE(#14, #12 - #13, 0.0)"
    );

    Ok(())
}

/// Aggregation through a meter chain: a meter measured via its single
/// (non-component) child meter when meter chains are enabled.
#[test]
fn test_aggregate_through_meter_chain() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    let meter1 = builder.meter();
    let meter2 = builder.meter();
    let bat_chain = builder.meter_bat_chain(1, 1);
    let pv_chain = builder.meter_pv_chain(1);

    builder.connect(grid, meter1);
    builder.connect(meter1, meter2);
    builder.connect(meter2, bat_chain);
    builder.connect(meter2, pv_chain);

    let graph = builder.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([meter1.component_id()]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#1, #2)");

    Ok(())
}

/// `measurement_points` substitutes a component's predecessor meter for the
/// component, but keeps a meterless component as its own measurement point.
///
/// Topology (ids): `Grid:0 → Meter:1 → Inverter:2 → Battery:3`, plus a
/// meterless `Grid:0 → Inverter:4 → Battery:5`.
#[test]
fn test_measurement_points() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let meter = builder.meter();
    let metered = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(grid, meter);
    builder.connect(meter, metered);
    builder.connect(metered, battery);

    let meterless = builder.battery_inverter();
    let meterless_battery = builder.battery();
    builder.connect(grid, meterless);
    builder.connect(meterless, meterless_battery);

    let graph = builder.build(None)?;

    // The metered inverter (2) resolves to its meter (1)...
    assert_eq!(
        super::resolve::measurement_points(
            &graph,
            &BTreeSet::from([metered.component_id()]),
            &BTreeSet::new()
        )?,
        vec![super::resolve::Measurement::Single(meter.component_id())],
    );
    // ...while the meterless inverter (4) is measured directly.
    assert_eq!(
        super::resolve::measurement_points(
            &graph,
            &BTreeSet::from([meterless.component_id()]),
            &BTreeSet::new()
        )?,
        vec![super::resolve::Measurement::Single(
            meterless.component_id()
        )],
    );
    Ok(())
}

/// A component with several parallel meter parents (a diamond) is measured
/// once, through the combined meter readings, rather than double-counted by
/// summing each meter independently.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2} → Inverter:3 → Battery:4`.
#[test]
fn test_aggregate_diamond() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let m1 = builder.meter();
    let m2 = builder.meter();
    let inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(grid, m1);
    builder.connect(grid, m2);
    builder.connect(m1, inverter);
    builder.connect(m2, inverter);
    builder.connect(inverter, battery);

    let graph = builder.build(None)?;
    let targets = BTreeSet::from([inverter.component_id()]);

    // The two meters collapse into one diamond term over the inverter.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Diamond {
            components: vec![inverter.component_id()],
            meters: vec![m1.component_id(), m2.component_id()],
        }],
    );

    // Meters primary: their sum, then the inverter, then a best-effort sum.
    assert_eq!(
        aggregate(
            &graph,
            targets.clone(),
            SourcePreference::MetersFirst { by_config: false }
        )?
        .to_string(),
        "COALESCE(#1 + #2, #3, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
    );
    // Components primary: the inverter, then the best-effort meter sum (the
    // exact meter sum is dominated by it and dropped).
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
        "COALESCE(#3, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
    );
    Ok(())
}

/// A meter with only component children drills in; one with multiple
/// successors that include a meter stands alone.
///
/// Topology (ids): `Meter:1 → {Inverter:2, Inverter:3}` and
/// `Meter:4 → {Meter:5, Inverter:6}`.
#[test]
fn test_stands_alone() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();

    let component_meter = builder.meter();
    let inv1 = builder.battery_inverter();
    let inv2 = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(grid, component_meter);
    builder.connect(component_meter, inv1);
    builder.connect(component_meter, inv2);
    builder.connect(inv1, battery);
    builder.connect(inv2, battery);

    let mixed_meter = builder.meter();
    let child_meter = builder.meter();
    let inv3 = builder.battery_inverter();
    let battery2 = builder.battery();
    builder.connect(grid, mixed_meter);
    builder.connect(mixed_meter, child_meter);
    builder.connect(mixed_meter, inv3);
    builder.connect(inv3, battery2);

    let graph = builder.build(None)?;
    assert!(!super::emit::stands_alone(
        &graph,
        component_meter.component_id(),
        SourcePreference::MetersFirst { by_config: false },
    )?);
    assert!(super::emit::stands_alone(
        &graph,
        mixed_meter.component_id(),
        SourcePreference::MetersFirstWithChains,
    )?);
    Ok(())
}

/// A stands-alone meter's fallback resolves child meters recursively, so
/// the term still evaluates when the meter and its sub-meter are offline
/// but the leaf components report.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 (PV),
/// Meter:4 → Inverter:5 → Battery:6}, Meter:7}` — the grid meter (Meter:1)
/// also feeds a load (Meter:7) so Meter:2 is an internal meter.
#[test]
fn test_stands_alone_total_through_child_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let battery_meter = builder.meter_bat_chain(1, 1);
    builder.connect(mixed_meter, battery_meter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([mixed_meter.component_id()]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    // The battery sub-meter (#4) is backed by its inverter (#5), not a
    // bare `#4` that would null the sum while #5 still reports.
    assert_eq!(
        expr.to_string(),
        "COALESCE(#2, COALESCE(#4, #5, 0.0) + COALESCE(#3, 0.0))",
    );
    Ok(())
}

/// A meter substitution over an asymmetric diamond resolves to the same
/// diamond regardless of which component id is lower. The seed fed by only
/// one of the parallel meters is rejected (its sibling is also fed from
/// outside that meter's parent set), and the sibling fed by the full meter
/// set forms the diamond, subsuming the seed's standalone point.
///
/// Topology: `Grid → GridMeter → {M_a, M_b}`, `M_a → {A, B}`, `M_b → B`,
/// built once with A's id lower and once with B's.
#[test]
fn test_substitution_asymmetric_diamond_order_independent() -> Result<(), Error> {
    for shared_id_first in [false, true] {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let m_a = builder.meter();
        let m_b = builder.meter();
        builder.connect(grid_meter, m_a);
        builder.connect(grid_meter, m_b);
        let first = builder.solar_inverter();
        let second = builder.solar_inverter();
        let (a, b) = if shared_id_first {
            (second, first)
        } else {
            (first, second)
        };
        builder.connect(m_a, a);
        builder.connect(m_a, b);
        builder.connect(m_b, b);

        let graph = builder.build(None)?;
        let targets = BTreeSet::from([a.component_id(), b.component_id()]);
        assert_eq!(
            super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
            vec![super::resolve::Measurement::Diamond {
                components: vec![4, 5],
                meters: vec![m_a.component_id(), m_b.component_id()],
            }],
            "shared_id_first: {shared_id_first}",
        );
        assert_eq!(
            aggregate(
                &graph,
                targets,
                SourcePreference::MetersFirst { by_config: false }
            )?
            .to_string(),
            "COALESCE(#2 + #3, #4 + #5, COALESCE(#2, 0.0) + COALESCE(#3, 0.0))",
            "shared_id_first: {shared_id_first}",
        );
    }
    Ok(())
}

/// A meter substitution is rejected when a covered sibling is itself one
/// of the seed's parent meters. Such a sibling passes the feed check
/// against itself, but its flow already runs through the other parent's
/// reading; a diamond built from this group would count that line twice —
/// once as a parallel meter and once as a covered component. The group
/// resolves through the seed whose parent set is just the outer meter:
/// the nested feed passes `reached_only_through`, so the outer meter covers
/// the whole group as a single point.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
/// Meter:4 → Inverter:5 → Battery:6}`, plus `Meter:2 → Inverter:5`
/// directly, so Inverter:5's parents are {2, 4} and Meter:4 is both a
/// parent meter and a sibling. Meter:1 also feeds a load (Meter:7).
#[test]
fn test_substitution_rejects_parent_meter_as_sibling() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let battery_meter = builder.meter();
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(mixed_meter, battery_meter);
    builder.connect(battery_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    builder.connect(mixed_meter, battery_inverter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // No diamond forms over {2, 4}: with Meter:4 also in the covered
    // components it would sum #2 + #4 while all of #4's flow is already
    // inside #2. Instead the seed under only Meter:2 covers the whole
    // group — Inverter:5's nested feed through Meter:4 still counts as
    // fed through Meter:2 — and the group is one point on that meter.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3, 4, 5]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(2)],
    );
    Ok(())
}

/// An off-limits parent meter is not substituted for the group it measures:
/// the targets resolve to their own points, as they would with no parent
/// meter at all.
#[test]
fn test_measurement_points_off_limits_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let solar_inverter = builder.solar_inverter();
    builder.connect(mixed_meter, solar_inverter);
    let chp = builder.chp();
    builder.connect(mixed_meter, chp);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    let targets = BTreeSet::from([solar_inverter.component_id(), chp.component_id()]);

    // Meter:2 measures exactly the pair, so it normally stands in for both.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(2)],
    );

    // Barred, it is not: each target becomes its own point.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::from([2]))?,
        vec![
            super::resolve::Measurement::Single(3),
            super::resolve::Measurement::Single(4)
        ],
    );
    Ok(())
}

/// A child meter that shares a component with a sibling meter (a diamond
/// one level below the meter being backed) stays a bare `#id` in the
/// children fallback sum. Recursing into both siblings would resolve each
/// to the shared component's reading and count it once per feed.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 (PV),
/// Meter:4, Meter:5}, Meter:7}`, with both Meter:4 and Meter:5 feeding
/// Inverter:6 (PV).
#[test]
fn test_child_meter_diamond_stays_bare() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv1 = builder.solar_inverter();
    builder.connect(mixed_meter, pv1);
    let m_a = builder.meter();
    let m_b = builder.meter();
    builder.connect(mixed_meter, m_a);
    builder.connect(mixed_meter, m_b);
    let pv2 = builder.solar_inverter();
    builder.connect(m_a, pv2);
    builder.connect(m_b, pv2);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // Meters #4 and #5 stay bare — resolving both to their shared
    // inverter (#6) would subtract its reading twice when the meters are
    // offline. The sum of bare readings is safe: each meter measures its
    // own feed line.
    let explained = aggregate(
        &graph,
        BTreeSet::from([mixed_meter.component_id()]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(
        explained.to_string(),
        "COALESCE(#2, #5 + #4 + COALESCE(#3, 0.0))",
    );
    // Those bare readings carry no fallback of their own, so neither the
    // children's sum nor the whole term is total. Both reasons must say so
    // instead of promising a 0.0 that is not there, and the sum's kind must
    // say it too: `BestEffortSum` would promise in machine-readable form
    // exactly what its own rationale denies.
    assert!(
        find_kind(&explained.explanation, &|kind| matches!(
            kind,
            ExplanationKind::BestEffortSum
        ))
        .is_none(),
        "a sum that can go missing must not be labelled best-effort",
    );
    assert_eq!(
        find_kind(&explained.explanation, &|kind| matches!(
            kind,
            ExplanationKind::MeterWithChildBackup
        ))
        .map(|node| node.rationale.as_str()),
        Some(
            "Meter #2 is measured by its own reading. If the reading goes \
             missing, the sum of its usable children backs it — though not \
             every term there carries a 0.0 fallback, so the term can still \
             go missing."
        ),
    );
    assert_eq!(
        find_kind(&explained.explanation, &|kind| matches!(
            kind,
            ExplanationKind::ChildrenSum
        ))
        .map(|node| node.rationale.as_str()),
        Some(
            "The sum of the meter's usable children. Not every term carries \
             a 0.0 fallback of its own, so the sum goes missing when a bare \
             reading does."
        ),
    );
    Ok(())
}

/// A meter over a mix of target components and other meters measures the
/// targets with the meter minus the sibling meters as the meter-side
/// source, ordered against the component readings by the policy.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3..7 (PV),
/// Meter:8}, Meter:9}` — a "PV + unspecified" meter next to an unspecified
/// sub-meter, both under a grid meter (Meter:1) that also feeds a separate
/// load (Meter:9), so Meter:2 is a genuine internal meter.
#[test]
fn test_aggregate_subtraction() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let inverters: Vec<_> = (0..5).map(|_| builder.solar_inverter()).collect();
    for inverter in &inverters {
        builder.connect(mixed_meter, *inverter);
    }
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    // A second branch off the grid meter (a building load) keeps Meter:1 a
    // grid meter and Meter:2 an internal meter, not a sole-child fallback
    // grid meter that would carry the site's unmodeled load.
    let load_meter = builder.meter();
    builder.connect(main_meter, load_meter);

    let graph = builder.build(None)?;
    let targets = BTreeSet::from([3, 4, 5, 6, 7]);

    // The group collapses into one subtraction term over the mixed meter.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![mixed_meter.component_id()],
            subtracted: vec![sub_meter.component_id()],
            components: vec![3, 4, 5, 6, 7],
        }],
    );

    // Meters primary: the difference, then the per-inverter sum.
    assert_eq!(
        aggregate(
            &graph,
            targets.clone(),
            SourcePreference::MetersFirst { by_config: false }
        )?
        .to_string(),
        concat!(
            "COALESCE(#2 - #8, ",
            "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
            "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
        ),
    );
    // Components primary: the exact sum, the difference, then best-effort.
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
        concat!(
            "COALESCE(#3 + #4 + #5 + #6 + #7, #2 - #8, ",
            "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
            "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
        ),
    );

    // The PV formula resolves to the subtraction term.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        concat!(
            "COALESCE(#3 + #4 + #5 + #6 + #7, #2 - #8, ",
            "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
            "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
        ),
    );
    // A partial group (one inverter without its mates) falls back to the
    // meter minus everything else under it — the working siblings and the
    // sub-meter.
    assert_eq!(
        graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
        "COALESCE(#3, #2 - #4 - #5 - #6 - #7 - #8, 0.0)",
    );
    Ok(())
}

/// Subtraction generalizes to a diamond: when a target's parents are several
/// parallel meters, the group is measured as the summed meter readings minus
/// the non-target siblings.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3} → {Inverter:4,
/// Inverter:5 (PV)}` — both internal meters feed both inverters (a diamond).
/// Meter:1 is the grid meter; Meter:2 and Meter:3 are internal.
#[test]
fn test_aggregate_subtraction_diamond() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let m_a = builder.meter();
    let m_b = builder.meter();
    builder.connect(grid_meter, m_a);
    builder.connect(grid_meter, m_b);
    let pv1 = builder.solar_inverter();
    let pv2 = builder.solar_inverter();
    for meter in [m_a, m_b] {
        builder.connect(meter, pv1);
        builder.connect(meter, pv2);
    }

    let graph = builder.build(None)?;
    let targets = BTreeSet::from([pv1.component_id()]);

    // One inverter behind the two parallel meters: the summed meter readings
    // minus the other inverter, with the inverter's own reading preferred.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![m_a.component_id(), m_b.component_id()],
            subtracted: vec![pv2.component_id()],
            components: vec![pv1.component_id()],
        }],
    );
    assert_eq!(
        aggregate(&graph, targets.clone(), SourcePreference::ComponentsFirst)?.to_string(),
        "COALESCE(#4, #2 + #3 - #5, 0.0)",
    );
    assert_eq!(
        aggregate(
            &graph,
            targets,
            SourcePreference::MetersFirst { by_config: false }
        )?
        .to_string(),
        "COALESCE(#2 + #3 - #5, #4, 0.0)",
    );

    // With both inverters targeted there is nothing to subtract, so it stays
    // a pure diamond.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([4, 5]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Diamond {
            components: vec![4, 5],
            meters: vec![2, 3],
        }],
    );
    Ok(())
}

#[test]
fn test_subtraction_diamond_subsumes_earlier_single() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let m_a = builder.meter();
    let m_b = builder.meter();
    builder.connect(grid_meter, m_a);
    builder.connect(grid_meter, m_b);
    let pv1 = builder.solar_inverter();
    let bat_inverter = builder.battery_inverter();
    let battery = builder.battery();
    let pv2 = builder.solar_inverter();
    builder.connect(m_a, pv1);
    builder.connect(m_a, bat_inverter);
    builder.connect(bat_inverter, battery);
    builder.connect(m_a, pv2);
    builder.connect(m_b, pv2);

    let graph = builder.build(None)?;
    let targets = BTreeSet::from([pv1.component_id(), pv2.component_id()]);

    // Asymmetric diamond: only `pv2` is fed by both parallel meters, so the
    // seed `pv1` resolves to a standalone `Single` first (its own
    // subtraction sees only `m_a`'s parent set and is disqualified by
    // `pv2`'s feed from `m_b`). The group's subtraction then covers `pv1`
    // and must subsume that point, or its readings would be counted twice.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![m_a.component_id(), m_b.component_id()],
            subtracted: vec![bat_inverter.component_id()],
            components: vec![pv1.component_id(), pv2.component_id()],
        }],
    );
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
        "COALESCE(#4 + #7, #2 + #3 - #5, COALESCE(#4, 0.0) + COALESCE(#7, 0.0))",
    );
    Ok(())
}

/// A diamond whose parallel parent meters are grid meters — directly under
/// the grid connection point, with mixed children so they are not component
/// meters — carries the site's unmodeled load, so the subtraction is
/// disqualified and the target falls back to its own reading, exercising the
/// per-meter grid-meter guard on the diamond path.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2} → {Inverter:3 (PV),
/// Meter:4}`, both parallel meters feeding the shared inverter and sub-meter.
#[test]
fn test_subtraction_diamond_disqualified_by_grid_meters() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let m1 = builder.meter();
    let m2 = builder.meter();
    builder.connect(grid, m1);
    builder.connect(grid, m2);
    let pv = builder.solar_inverter();
    let sub_meter = builder.meter();
    for meter in [m1, m2] {
        builder.connect(meter, pv);
        builder.connect(meter, sub_meter);
    }

    let graph = builder.build(None)?;
    // The parallel parents are grid meters (mixed children, under the grid),
    // so the inverter falls back to its own reading rather than the
    // meter-sum-minus-sub-meter difference.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// The subtracted siblings may be components rather than meters: a single
/// target inverter (e.g. one that is unreachable over the network) falls
/// back to the meter minus its working siblings, and a category's
/// inverters next to another category's inverter fall back to the meter
/// minus that inverter.
#[test]
fn test_aggregate_subtraction_component_siblings() -> Result<(), Error> {
    // One inverter out of three: the meter minus the others as fallback.
    // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → Inverter:3..5 (PV)`.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let pv_meter = builder.meter_pv_chain(3);
    builder.connect(grid, main_meter);
    builder.connect(main_meter, pv_meter);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
        "COALESCE(#3, #2 - #4 - #5, 0.0)",
    );

    // A PV inverter next to a battery inverter: each category is the
    // meter minus the other category's inverter.
    // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 (PV),
    // Inverter:4 → Battery:5}, Meter:6}` — the grid meter (Meter:1) also
    // feeds a load (Meter:6) so Meter:2 is an internal meter.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let battery_inverter = builder.inv_bat_chain(1);
    builder.connect(mixed_meter, battery_inverter);
    let load_meter = builder.meter();
    builder.connect(main_meter, load_meter);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, #2 - #4, 0.0)",
    );
    assert_eq!(
        graph.battery_formula(None)?.to_string(),
        "COALESCE(#4, #2 - #3, 0.0)",
    );
    Ok(())
}

/// The subtraction also applies when the sibling meter is a component
/// meter, in either order: PV inverters next to a battery sub-meter get
/// `mixed - battery_meter` as their meter-side source, and battery
/// inverters next to a PV sub-meter get `mixed - pv_meter`. The
/// sub-meter's own category formula is unaffected.
#[test]
fn test_aggregate_subtraction_component_sub_meter() -> Result<(), Error> {
    // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3,4 (PV),
    // Meter:5 → Inverter:6 → Battery:7}, Meter:8}` — the grid meter
    // (Meter:1) also feeds a load (Meter:8) so Meter:2 is an internal meter.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let pv1 = builder.solar_inverter();
    let pv2 = builder.solar_inverter();
    builder.connect(mixed_meter, pv1);
    builder.connect(mixed_meter, pv2);
    let battery_meter = builder.meter_bat_chain(1, 1);
    builder.connect(mixed_meter, battery_meter);
    let load_meter = builder.meter();
    builder.connect(main_meter, load_meter);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3 + #4, #2 - #5, COALESCE(#3, 0.0) + COALESCE(#4, 0.0))",
    );
    assert_eq!(
        graph.battery_formula(None)?.to_string(),
        "COALESCE(#6, #5, 0.0)",
    );

    // The reverse order: battery inverters under the mixed meter, the PV
    // system behind the sub-meter.
    //
    // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 →
    // Battery:4, Meter:5 → Inverter:6 (PV)}, Meter:7}` — the grid meter
    // (Meter:1) also feeds a load (Meter:7) so Meter:2 is an internal meter.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let battery_inverter = builder.inv_bat_chain(1);
    builder.connect(mixed_meter, battery_inverter);
    let pv_meter = builder.meter_pv_chain(1);
    builder.connect(mixed_meter, pv_meter);
    let load_meter = builder.meter();
    builder.connect(main_meter, load_meter);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.battery_formula(None)?.to_string(),
        "COALESCE(#3, #2 - #5, 0.0)",
    );
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#6, #5, 0.0)");
    Ok(())
}

/// Shapes where the parent meter's reading can't be split cleanly fall
/// back to measuring the targets directly:
/// - a non-target sibling with no usable reading (its share is unknown);
/// - a sibling meter leading to other targets (they are measured on their
///   own, so subtracting them would drop them from the total);
/// - a parent meter directly under the grid connection point (it carries
///   the site's unmodeled consumer load);
/// - a sibling meter that is also fed from outside the parent.
#[test]
fn test_subtraction_disqualifiers() -> Result<(), Error> {
    // Non-target sibling with no usable reading (a hybrid inverter is not
    // a measurable component).
    // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    // Inverter:4 (hybrid) → Battery:5}`.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let hybrid = builder.add_component(crate::ComponentCategory::Inverter(
        crate::InverterType::Hybrid,
    ));
    let battery = builder.battery();
    builder.connect(mixed_meter, hybrid);
    builder.connect(hybrid, battery);

    let graph = builder.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Sibling meter leading to another target.
    // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    // Meter:4 → Inverter:5 (PV)}`.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let pv_meter = builder.meter_pv_chain(1);
    builder.connect(mixed_meter, pv_meter);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, 0.0) + COALESCE(#5, #4, 0.0)",
    );

    // Parent meter directly under the grid connection point.
    // Topology (ids): `Grid:0 → Meter:1 → {Inverter:2 (PV), Meter:3}`.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let pv = builder.solar_inverter();
    builder.connect(grid_meter, pv);
    let sub_meter = builder.meter();
    builder.connect(grid_meter, sub_meter);

    let graph = builder.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, 0.0)");

    // Parent meter is a fallback grid meter: the sole child of the grid
    // meter, so it stands in for the grid connection point and likewise
    // carries the site's unmodeled consumer load.
    // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    // Meter:4}`, where Meter:2 is Meter:1's only child.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let fallback_grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, fallback_grid_meter);
    let pv = builder.solar_inverter();
    builder.connect(fallback_grid_meter, pv);
    let sub_meter = builder.meter();
    builder.connect(fallback_grid_meter, sub_meter);

    let graph = builder.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Sibling meter also fed from outside the parent.
    // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    // Meter:4}`, plus `Meter:1 → Meter:4`.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    builder.connect(main_meter, sub_meter);

    let graph = builder.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Covered target also fed from outside the parent meter — here directly
    // by the grid, through no meter at all.
    // The extra inflow is not in the parent-meter reading, so the
    // meter-minus-siblings difference would undercount the group; the
    // targets are measured directly instead. Such a topology only arises
    // once neighbour validation is bypassed (the grid rule would otherwise
    // reject a second predecessor on a grid successor), so the guard is a
    // backstop for exactly that case.
    // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:4 (PV),
    // Inverter:5 (PV), Meter:6}, Meter:3}`, plus `Grid:0 → Inverter:5`.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let main_meter = builder.meter();
    let mixed_meter = builder.meter();
    let other_meter = builder.meter();
    builder.connect(grid, main_meter);
    builder.connect(main_meter, mixed_meter);
    builder.connect(main_meter, other_meter);
    let pv = builder.solar_inverter();
    let pv_external = builder.solar_inverter();
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, pv);
    builder.connect(mixed_meter, pv_external);
    builder.connect(mixed_meter, sub_meter);
    builder.connect(grid, pv_external);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#4, 0.0) + COALESCE(#5, 0.0)",
    );
    Ok(())
}

/// A child meter that feeds a sibling of its own contributes no term to
/// the children fallback sum: the flow it measures is already inside the
/// fed sibling's reading, so a bare `#id` next to that sibling's term
/// would count the shared line twice.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
/// Meter:4 → Inverter:5 → Battery:6}`, plus `Meter:2 → Inverter:5`
/// directly, and a load (Meter:7) under the grid meter.
#[test]
fn test_children_fallback_drops_feeder_of_sibling() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let battery_meter = builder.meter();
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(mixed_meter, battery_meter);
    builder.connect(battery_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    builder.connect(mixed_meter, battery_inverter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // No bare `#4` next to `COALESCE(#5, 0.0)`: inverter #5's reading
    // already contains the flow through meter #4.
    let explained = aggregate(
        &graph,
        BTreeSet::from([mixed_meter.component_id()]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(
        explained.to_string(),
        "COALESCE(#2, COALESCE(#5, 0.0) + COALESCE(#3, 0.0))",
    );
    // The dropped meter is recorded as a silent part naming the fed sibling.
    let note = find_kind(&explained.explanation, &|kind| {
        matches!(kind, ExplanationKind::ChildSkipped { .. })
    })
    .expect("skip note for meter #4");
    assert_eq!(
        note.kind,
        ExplanationKind::ChildSkipped {
            feeds: vec![battery_inverter.component_id()],
            fed_from: vec![],
        }
    );
    assert_eq!(note.component_ids, vec![battery_meter.component_id()]);
    assert_eq!(note.rendered(), None);
    Ok(())
}

/// A child that feeds a sibling through a nested meter is dropped from
/// the children fallback too, not only a direct feeder: the flow through
/// the nested chain is already inside the fed sibling's reading.
#[test]
fn test_children_fallback_drops_transitive_feeder() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let sub_a = builder.meter();
    builder.connect(mixed_meter, sub_a);
    let sub_b = builder.meter();
    builder.connect(sub_a, sub_b);
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(sub_b, battery_inverter);
    builder.connect(mixed_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // No term for meter #3 next to the inverter's reading-or-0: inverter
    // #5's reading already contains the flow through meters #3 and #4.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([mixed_meter.component_id()]),
            SourcePreference::MetersFirst { by_config: false },
        )?
        .to_string(),
        "COALESCE(#2, #5, 0.0)",
    );
    Ok(())
}

/// A device child that feeds a sibling meter is dropped from the children
/// fallback: the sibling meter's reading already contains its flow. The
/// fed meter is dropped too — it is fed from outside the measured meter,
/// so its reading holds more than the measured meter passes. Such edges
/// exist only when validation failures are allowed.
#[test]
fn test_children_fallback_drops_device_feeder() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(mixed_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    builder.connect(battery_inverter, sub_meter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    // No child term at all: inverter #3 feeds sibling #5, and meter #5
    // is fed by #3 from outside meter #2's line.
    let explained = aggregate(
        &graph,
        BTreeSet::from([mixed_meter.component_id()]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(explained.to_string(), "#2");
    // Both skips are recorded, each naming its counterpart: the feeder
    // names the fed sibling, the fed meter names its outside feed.
    let feeder = find_kind(&explained.explanation, &|kind| {
        kind == &ExplanationKind::ChildSkipped {
            feeds: vec![sub_meter.component_id()],
            fed_from: vec![],
        }
    })
    .expect("skip note for inverter #3");
    assert_eq!(feeder.component_ids, vec![battery_inverter.component_id()]);
    let fed = find_kind(&explained.explanation, &|kind| {
        kind == &ExplanationKind::ChildSkipped {
            feeds: vec![],
            fed_from: vec![battery_inverter.component_id()],
        }
    })
    .expect("skip note for meter #5");
    assert_eq!(fed.component_ids, vec![sub_meter.component_id()]);
    Ok(())
}

/// A child fed by two parallel meters backs neither meter: its reading
/// holds both meters' lines, so it would overstate each. Each meter is
/// measured bare; a formula that targets the component itself measures
/// the pair as a diamond instead.
#[test]
fn test_children_fallback_drops_child_shared_with_parallel_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let meter_a = builder.meter();
    builder.connect(grid_meter, meter_a);
    let meter_b = builder.meter();
    builder.connect(grid_meter, meter_b);
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(meter_a, battery_inverter);
    builder.connect(meter_b, battery_inverter);
    builder.connect(battery_inverter, battery);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // No `COALESCE(#_, #4, 0.0)` terms: inverter #4's reading would be
    // subtracted once per parallel meter, counting its power twice.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([meter_a.component_id(), meter_b.component_id()]),
            SourcePreference::MetersFirst { by_config: false },
        )?
        .to_string(),
        "#2 + #3",
    );
    Ok(())
}

/// A sub-meter whose `Single` point was dropped for a covering group
/// stays claimed: a later target below it must not substitute it back
/// in — its flow is already inside the covering point.
#[test]
fn test_subsumed_sub_meter_stays_claimed() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let pv = builder.solar_inverter();
    builder.connect(grid_meter, pv);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let sub_meter = builder.meter();
    let chp = builder.chp();
    builder.connect(mixed_meter, sub_meter);
    builder.connect(mixed_meter, chp);
    let nested_chp = builder.chp();
    builder.connect(sub_meter, nested_chp);

    let graph = builder.build(None)?;
    // Seed order: sub-meter #4 first gets its own `Single`; CHP #5 then
    // substitutes mixed meter #3 in for the whole group and drops that
    // point. Nested CHP #6 must not claim #4 again — one term, not two.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([
                sub_meter.component_id(),
                chp.component_id(),
                nested_chp.component_id(),
            ]),
            SourcePreference::MetersFirst { by_config: false },
        )?
        .to_string(),
        "COALESCE(#3, COALESCE(#5, 0.0) + COALESCE(#4, #6, 0.0))",
    );
    Ok(())
}

/// A grid meter is never backed by its children's sum, in the drilling
/// path too: its reading carries the site's unmodeled consumer load,
/// which no sum of its children accounts for. A fallback grid meter (the
/// sole child of a grid meter) still backs the outer meter as a chain.
#[test]
fn test_grid_meter_never_backed_by_children() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let fallback_grid_meter = builder.meter();
    builder.connect(grid_meter, fallback_grid_meter);
    let pv = builder.solar_inverter();
    let chp = builder.chp();
    builder.connect(fallback_grid_meter, pv);
    builder.connect(fallback_grid_meter, chp);

    let graph = builder.build(None)?;
    // The chain falls back from #1 to #2, but never to the device sum:
    // the devices are only the producers, so their sum would report a
    // false value while both meters are offline.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([grid_meter.component_id()]),
            SourcePreference::MetersFirstWithChains,
        )?
        .to_string(),
        "COALESCE(#1, #2)",
    );
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([fallback_grid_meter.component_id()]),
            SourcePreference::MetersFirst { by_config: false },
        )?
        .to_string(),
        "#2",
    );
    Ok(())
}

/// A subtracted sibling fed by another subtracted sibling is not counted
/// twice. The sub-meter measures flow that is already inside the fed
/// inverter's own reading. When the sub-meter feeds only subtracted
/// siblings, it is dropped from the difference; when it also feeds
/// something else, the difference can't be split and the subtraction
/// does not apply.
#[test]
fn test_subtraction_covered_feeder() -> Result<(), Error> {
    // The sub-meter's only child is the battery inverter, which is also a
    // direct child of the mixed meter. Only the inverter is subtracted.
    // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    // Meter:4, Inverter:5 → Battery:6}`, plus `Meter:4 → Inverter:5`, and
    // a load (Meter:7) under the grid meter.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(mixed_meter, battery_inverter);
    builder.connect(sub_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, #2 - #5, 0.0)",
    );

    // The sub-meter also feeds a CHP of its own: dropping it would lose
    // the CHP's flow, keeping it would count the inverter's flow twice.
    // The subtraction does not apply.
    // Topology (ids): as above, plus `Meter:4 → CHP:7`; the load meter
    // is Meter:8.
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(mixed_meter, battery_inverter);
    builder.connect(sub_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    let chp = builder.chp();
    builder.connect(sub_meter, chp);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// A meter fed by both the grid connection point and another meter (a
/// shape that only builds when validation failures are allowed) is a grid
/// meter no matter in which order the edges were added. Every predecessor
/// is checked, so the formula does not flip with edge order.
///
/// Topology (ids): `Grid:0 → Meter:1`, `Meter:2 → {Inverter:3 (PV),
/// Meter:4}`, with `Meter:2` fed by both `Grid:0` and `Meter:1`.
#[test]
fn test_grid_meter_second_feed_order_independent() -> Result<(), Error> {
    for grid_edge_first in [true, false] {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let other_meter = builder.meter();
        builder.connect(grid, other_meter);
        let meter = builder.meter();
        if grid_edge_first {
            builder.connect(grid, meter);
            builder.connect(other_meter, meter);
        } else {
            builder.connect(other_meter, meter);
            builder.connect(grid, meter);
        }
        let pv = builder.solar_inverter();
        builder.connect(meter, pv);
        let sub_meter = builder.meter();
        builder.connect(meter, sub_meter);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_component_validation_failures(true)
                .build(),
        ))?;
        // The grid-fed meter is never a subtraction source.
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#3, 0.0)",
            "grid_edge_first: {grid_edge_first}",
        );
    }
    Ok(())
}

/// A cycle that is not reachable from the root survives validation when
/// unconnected components are allowed (the acyclicity walk starts at the
/// root). Formula generation must still stop on such a graph instead of
/// recursing forever through the cycle's predecessors.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → Inverter:3 (PV)`, plus
/// an off-root cycle `Meter:4 ↔ Meter:5` with `Meter:4 → Meter:2` and
/// `Meter:4 → Inverter:3`.
#[test]
fn test_off_root_cycle_terminates() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let meter = builder.meter();
    builder.connect(grid_meter, meter);
    let pv = builder.solar_inverter();
    builder.connect(meter, pv);
    let x = builder.meter();
    let y = builder.meter();
    builder.connect(x, y);
    builder.connect(y, x);
    builder.connect(x, meter);
    builder.connect(x, pv);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .allow_unconnected_components(true)
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    // The exact formula does not matter on an invalid graph; generating
    // it just must terminate.
    graph.pv_formula(None)?;
    Ok(())
}

/// Pins that a subtraction stays a subtraction when the redundant-feeder
/// pruning empties its subtracted set. Every subtracted sibling must then
/// have all its successors inside the subtracted set, which needs a cycle
/// among the siblings — so this input exists only on graphs kept alive by
/// the validation allow-flags. The point still carries the subtraction
/// shape (parent-meter sum, nothing subtracted), not a meter
/// substitution.
///
/// Topology (ids): `Grid:0 → Meter:1`, and an off-root `Meter:2 →
/// {Inverter:3 (PV), Meter:4, Meter:5}` with `Meter:4 ↔ Meter:5`.
#[test]
fn test_pruned_empty_subtraction_keeps_shape() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let meter = builder.meter();
    let pv = builder.solar_inverter();
    builder.connect(meter, pv);
    let a = builder.meter();
    let b = builder.meter();
    builder.connect(meter, a);
    builder.connect(meter, b);
    builder.connect(a, b);
    builder.connect(b, a);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .allow_unconnected_components(true)
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![2],
            subtracted: vec![],
            components: vec![3],
        }],
    );
    Ok(())
}

/// Pins the resolver's claim bookkeeping for meter targets.
///
/// Only the consumer formula puts meters in the target set, and its
/// target collection stops at the first component chain, so a target is
/// never nested below another target meter. The scenarios here go beyond
/// that caller contract on purpose: they pin how the claim set behaves
/// today, so a restructuring that changes it is noticed. A deliberate
/// behavior change may update these expectations.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
/// Inverter:4 (PV), Inverter:5 → Battery:6}`, and a load (Meter:7) under
/// the grid meter.
#[test]
fn test_claim_semantics_meter_targets() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let pv1 = builder.solar_inverter();
    builder.connect(mixed_meter, pv1);
    let pv2 = builder.solar_inverter();
    builder.connect(mixed_meter, pv2);
    let battery_inverter = builder.inv_bat_chain(1);
    builder.connect(mixed_meter, battery_inverter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);
    let graph = builder.build(None)?;

    // The meter target claims itself as a `Single`. The inverter's
    // subtraction is then rejected — its parent meter is already claimed
    // by a point that measures more than the inverter's group — and the
    // seed falls back to a standalone point.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([2, 3]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(2),
            super::resolve::Measurement::Single(3)
        ],
    );
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([2, 3, 4]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(2),
            super::resolve::Measurement::Single(3),
            super::resolve::Measurement::Single(4)
        ],
    );

    // With every child of the mixed meter targeted, the substitution
    // resolves the group onto the already-claimed meter: the group merges
    // into the existing point and emits nothing new.
    assert_eq!(
        super::resolve::measurement_points(
            &graph,
            &BTreeSet::from([2, 3, 4, 5]),
            &BTreeSet::new()
        )?,
        vec![super::resolve::Measurement::Single(2)],
    );
    Ok(())
}

/// Pins that a subsumed point stays claimed: a later seed must not claim
/// the same node again, because its flow is already inside the covering
/// point. The input has a target nested below a target meter, which no
/// current caller produces (see [`test_claim_semantics_meter_targets`] on
/// the caller contract); the nested target then resolves to no point of
/// its own.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Meter:3 → Inverter:5 →
/// Battery:6, Inverter:4 (PV)}`, and a load (Meter:7) under the grid
/// meter.
#[test]
fn test_subsumed_claim_stays_claimed() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let battery_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(sub_meter, battery_inverter);
    builder.connect(battery_inverter, battery);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);
    let graph = builder.build(None)?;

    // Seed 3 emits Single(3); seed 4's substitution onto Meter:2 subsumes
    // it and drops that point. Meter:3 stays claimed, so seed 5's
    // substitution onto it emits nothing — its flow is already inside
    // Single(2).
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3, 4, 5]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(2)],
    );
    Ok(())
}

/// A target sibling that is a meter disqualifies the subtraction: the
/// parent's reading can't be split between the meter target and the other
/// targets. Both end up as standalone points.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Meter:3, Inverter:4
/// (PV), Inverter:5 → Battery:6}`, and a load (Meter:7) under the grid
/// meter.
#[test]
fn test_subtraction_rejects_target_meter_sibling() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, sub_meter);
    let pv = builder.solar_inverter();
    builder.connect(mixed_meter, pv);
    let battery_inverter = builder.inv_bat_chain(1);
    builder.connect(mixed_meter, battery_inverter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);
    let graph = builder.build(None)?;

    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3, 4]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(3),
            super::resolve::Measurement::Single(4)
        ],
    );
    Ok(())
}

/// Two diamonds can never share a parallel meter: a shared meter means
/// one group's components are also fed by a meter that feeds the other
/// group, so the coverage check rejects both groups and each target stays
/// a standalone point. This pins that the resolver's meter claims never
/// meet an overlapping second diamond.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3, Meter:4}`, with
/// `Meter:2 → Inverter:5`, `Meter:3 → {Inverter:5, Inverter:6}`,
/// `Meter:4 → Inverter:6` (both PV).
#[test]
fn test_no_group_across_overlapping_diamonds() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let m_a = builder.meter();
    let m_b = builder.meter();
    let m_c = builder.meter();
    builder.connect(grid_meter, m_a);
    builder.connect(grid_meter, m_b);
    builder.connect(grid_meter, m_c);
    let pv1 = builder.solar_inverter();
    let pv2 = builder.solar_inverter();
    builder.connect(m_a, pv1);
    builder.connect(m_b, pv1);
    builder.connect(m_b, pv2);
    builder.connect(m_c, pv2);
    let graph = builder.build(None)?;

    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([5, 6]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(5),
            super::resolve::Measurement::Single(6)
        ],
    );
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#5, 0.0) + COALESCE(#6, 0.0)",
    );
    Ok(())
}

/// The subtraction-diamond subsume resolves to the same point no matter
/// which component id is lower — the twin of
/// [`test_substitution_asymmetric_diamond_order_independent`] for the
/// subtraction path. When the shared component's id is lower, the group
/// forms directly on the first seed and there is no standalone point to
/// subsume; the result is identical.
///
/// Topology: `Grid → GridMeter → {M_a, M_b}`, `M_a → {PV_x, BatInverter →
/// Battery, PV_shared}`, `M_b → PV_shared`, built once with PV_x's id
/// lower and once with PV_shared's.
#[test]
fn test_subtraction_diamond_subsume_order_independent() -> Result<(), Error> {
    for shared_id_first in [false, true] {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let m_a = builder.meter();
        let m_b = builder.meter();
        builder.connect(grid_meter, m_a);
        builder.connect(grid_meter, m_b);
        let first = builder.solar_inverter();
        let bat_inverter = builder.battery_inverter();
        let battery = builder.battery();
        let second = builder.solar_inverter();
        let (pv_x, pv_shared) = if shared_id_first {
            (second, first)
        } else {
            (first, second)
        };
        builder.connect(m_a, pv_x);
        builder.connect(m_a, bat_inverter);
        builder.connect(bat_inverter, battery);
        builder.connect(m_a, pv_shared);
        builder.connect(m_b, pv_shared);
        let graph = builder.build(None)?;

        let targets = BTreeSet::from([pv_x.component_id(), pv_shared.component_id()]);
        assert_eq!(
            super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
            vec![super::resolve::Measurement::Subtraction {
                parent_meters: vec![m_a.component_id(), m_b.component_id()],
                subtracted: vec![bat_inverter.component_id()],
                components: {
                    let mut components = vec![pv_x.component_id(), pv_shared.component_id()];
                    components.sort_unstable();
                    components
                },
            }],
            "shared_id_first: {shared_id_first}",
        );
    }
    Ok(())
}

/// A component that provides no telemetry is dropped as a measurement source
/// but still measured through its meter, and still classifies that meter.
///
/// Topology (ids): `Grid:0 → Meter:1 → PVMeter:2 → {PV:3, PV:4 (no
/// telemetry)}`.
#[test]
fn test_no_telemetry_component_measured_via_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let pv_meter = builder.meter();
    builder.connect(grid_meter, pv_meter);
    let pv = builder.solar_inverter();
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(pv_meter, pv);
    builder.connect(pv_meter, pv_no_telemetry);

    let graph = builder.build(None)?;

    // The no-telemetry inverter #4 is not a measurement source: the meter #2
    // measures it, and only the reporting inverter #3 backs the meter.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, #3, 0.0)",);

    // It still classifies the meter as a PV meter.
    assert!(graph.is_pv_meter(pv_meter.component_id())?);
    assert!(graph.is_pv_chain(pv_no_telemetry.component_id())?);
    Ok(())
}

/// A reporting meter whose children all lack telemetry stands alone on its
/// reading, backed by 0.0 so the term stays total.
///
/// Topology (ids): `Grid:0 → Meter:1 → PVMeter:2 → PV:3 (no telemetry)`,
/// plus `Meter:1 → Meter:4` so Meter:2 isn't a sole-child fallback grid
/// meter.
#[test]
fn test_no_telemetry_children_meter_stays_total() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let pv_meter = builder.meter();
    builder.connect(grid_meter, pv_meter);
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(pv_meter, pv_no_telemetry);
    let other_meter = builder.meter();
    builder.connect(grid_meter, other_meter);

    let graph = builder.build(None)?;
    // Meter #2's reading is the only source for PV:3, but the term must not
    // go null with it.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, 0.0)");
    // The excluded child is recorded as a silent part naming the cause.
    let explained = super::super::generators::category::category_formula_explained(
        &graph,
        None,
        crate::component_category::CategoryPredicates::is_pv_inverter,
        "a PV inverter",
        false,
    )?;
    let note = find_kind(&explained.explanation, &|kind| {
        kind == &ExplanationKind::NoTelemetryZero
    })
    .expect("silent note for PV:3");
    assert_eq!(note.component_ids, vec![pv_no_telemetry.component_id()]);
    assert_eq!(note.rendered(), None);
    assert_eq!(
        note.rationale,
        "Child PV inverter #3 is in control-only mode and provides no \
         telemetry: it has no reading to add, so the child sums leave it \
         out. The meter's own reading still covers its flow."
    );
    Ok(())
}

/// A skip note survives the components-first ladder that drops the
/// best-effort sum (a single kept child): the drill node still records the
/// skipped child and its outside feed.
///
/// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3}`, with
/// `{Meter:2, Meter:3} → Inverter:4 → Battery:5` (a parallel feed),
/// `Meter:2 → Inverter:6 → Battery:7`, and a load (Meter:8) under the grid
/// meter.
#[test]
fn test_skip_note_survives_single_kept_child() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let meter_a = builder.meter();
    builder.connect(grid_meter, meter_a);
    let meter_b = builder.meter();
    builder.connect(grid_meter, meter_b);
    let shared_inverter = builder.battery_inverter();
    let battery = builder.battery();
    builder.connect(meter_a, shared_inverter);
    builder.connect(meter_b, shared_inverter);
    builder.connect(shared_inverter, battery);
    let own_inverter = builder.battery_inverter();
    let own_battery = builder.battery();
    builder.connect(meter_a, own_inverter);
    builder.connect(own_inverter, own_battery);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    let explained = aggregate(
        &graph,
        BTreeSet::from([meter_a.component_id()]),
        SourcePreference::ComponentsFirst,
    )?;
    // A single kept child (#6): the ladder is exact / meter / 0.0, with no
    // best-effort sum — the note must not vanish with it.
    assert_eq!(explained.to_string(), "COALESCE(#6, #2, 0.0)");
    let note = find_kind(&explained.explanation, &|kind| {
        kind == &ExplanationKind::ChildSkipped {
            feeds: vec![],
            fed_from: vec![meter_b.component_id()],
        }
    })
    .expect("skip note for the parallel-fed inverter");
    assert_eq!(note.component_ids, vec![shared_inverter.component_id()]);
    Ok(())
}

/// A no-telemetry component with no meter to measure it contributes nothing
/// (its reading is never emitted).
///
/// Topology (ids): `Grid:0 → PV:1 (no telemetry)`.
#[test]
fn test_no_telemetry_meterless_component_dropped() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::Inactive,
    );
    builder.connect(grid, pv_no_telemetry);

    let graph = builder.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "0.0");
    // The `0.0` names no component, so the node must carry the id itself:
    // without it the component vanishes from the explanation tree.
    let explained = super::super::generators::category::category_formula_explained(
        &graph,
        None,
        crate::component_category::CategoryPredicates::is_pv_inverter,
        "a PV inverter",
        false,
    )?;
    let note = find_kind(&explained.explanation, &|kind| {
        kind == &ExplanationKind::NoTelemetryZero
    })
    .expect("no-telemetry note for PV:1");
    assert_eq!(note.component_ids, vec![pv_no_telemetry.component_id()]);
    assert_eq!(note.rendered().as_deref(), Some("0.0"));
    Ok(())
}

/// In a diamond, a no-telemetry target drops out of the component-side term,
/// leaving the meters as the sole source of the group total.
///
/// Topology (ids): `Grid:0 → {Meter:1, Meter:2} → {PV:3, PV:4 (no
/// telemetry)}`.
#[test]
fn test_no_telemetry_in_diamond() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let m1 = builder.meter();
    let m2 = builder.meter();
    builder.connect(grid, m1);
    builder.connect(grid, m2);
    let pv = builder.solar_inverter();
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    for meter in [m1, m2] {
        builder.connect(meter, pv);
        builder.connect(meter, pv_no_telemetry);
    }

    let graph = builder.build(None)?;
    // The component sum can't represent the group (only #3 reports), so the
    // meters are the total; #4 is absent.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#1 + #2, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
    );
    Ok(())
}

/// In a subtraction, a no-telemetry target drops out of the component-side
/// terms, so the meter-minus-siblings difference is the primary source.
///
/// Topology (ids): `Grid:0 → Meter:1 → MixedMeter:2 → {PV:3, PV:4 (no
/// telemetry), Meter:5}`, plus `Meter:1 → Meter:6` to keep Meter:2 internal.
#[test]
fn test_no_telemetry_in_subtraction() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, pv);
    builder.connect(mixed_meter, pv_no_telemetry);
    builder.connect(mixed_meter, sub_meter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // Only #3 reports, so the exact component sum can't be primary: the
    // difference #2 - #5 is, with #3 as the degraded fallback. #4 is absent.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#2 - #5, #3, 0.0)",
    );
    Ok(())
}

/// In a subtraction where every target lacks telemetry, no component reading
/// backs the difference, so it falls back to `0.0` rather than going null
/// when the parent meter is missing.
///
/// Topology (ids): `Grid:0 → Meter:1 → MixedMeter:2 → {PV:3 (no telemetry),
/// PV:4 (no telemetry), Meter:5}`, plus `Meter:1 → Meter:6` to keep Meter:2
/// internal.
#[test]
fn test_all_no_telemetry_in_subtraction() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, pv);
    builder.connect(mixed_meter, pv_no_telemetry);
    builder.connect(mixed_meter, sub_meter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // Neither #3 nor #4 reports, so the difference #2 - #5 is the sole
    // source, backed by 0.0 so the term stays total. #3 and #4 are absent.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#2 - #5, 0.0)",
    );
    Ok(())
}

/// Under `disable_fallback_components`, a no-telemetry target is dropped
/// from the raw component sum.
///
/// Topology (ids): `Grid:0 → Meter:1 → {PV:2, PV:3 (no telemetry)}`.
#[test]
fn test_no_telemetry_disable_fallback() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let meter = builder.meter();
    builder.connect(grid, meter);
    let pv = builder.solar_inverter();
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(meter, pv);
    builder.connect(meter, pv_no_telemetry);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "#2");
    Ok(())
}

/// Under `disable_fallback_components`, when every target is dropped for lack
/// of telemetry the term is kept total with a `0.0` (not an empty formula).
///
/// Topology (ids): `Grid:0 → Meter:1 → PV:2 (no telemetry)`.
#[test]
fn test_no_telemetry_disable_fallback_all_dropped() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let meter = builder.meter();
    builder.connect(grid, meter);
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(meter, pv_no_telemetry);

    let graph = builder.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "0.0");
    Ok(())
}

/// A meter that provides no telemetry has no reading of its own: a component
/// under it is measured directly, and drilling the meter itself never emits
/// its `#id`.
///
/// Topology (ids): `Grid:0 → Meter:1 → PVMeter:2 (no telemetry) → PV:3`,
/// plus `Meter:1 → Meter:4` so Meter:2 isn't a sole-child fallback grid meter.
#[test]
fn test_no_telemetry_meter_dropped() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let pv_meter =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    builder.connect(grid_meter, pv_meter);
    let pv = builder.solar_inverter();
    builder.connect(pv_meter, pv);
    let other_meter = builder.meter();
    builder.connect(grid_meter, other_meter);

    let graph = builder.build(None)?;

    // The PV inverter #3 is under the no-telemetry meter #2, so it is measured
    // directly; #2 never appears.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Drilling the no-telemetry meter directly measures it by its children,
    // never emitting its own #2.
    let expr = aggregate(
        &graph,
        BTreeSet::from([pv_meter.component_id()]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// A non-target sibling that provides no telemetry disqualifies the
/// meter-minus-siblings subtraction (its share can't be subtracted), so the
/// targets are measured directly instead.
///
/// Topology (ids): `Grid:0 → Meter:1 → MixedMeter:2 → {PV:3, PV:4 (no
/// telemetry), Meter:5}`, plus `Meter:1 → Meter:6`. Target only PV:3.
#[test]
fn test_no_telemetry_sibling_disqualifies_subtraction() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    let mixed_meter = builder.meter();
    builder.connect(grid, grid_meter);
    builder.connect(grid_meter, mixed_meter);
    let pv = builder.solar_inverter();
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    let sub_meter = builder.meter();
    builder.connect(mixed_meter, pv);
    builder.connect(mixed_meter, pv_no_telemetry);
    builder.connect(mixed_meter, sub_meter);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;

    // Targeting only PV:3: the non-target sibling PV:4 has no telemetry, so
    // the subtraction #2 - ... can't isolate the group — PV:3 is measured
    // directly, and neither #2, #4, nor #5 appear.
    let expr = aggregate(
        &graph,
        BTreeSet::from([pv.component_id()]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// A child meter that feeds a sibling without telemetry keeps its own term:
/// the sibling has no reading to double count, and the feeder's reading is
/// the only source for that line.
///
/// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Meter:3, Inverter:4 (PV,
/// no telemetry)}`, plus `Meter:3 → Inverter:4`, and a load (Meter:5) under
/// the grid meter.
#[test]
fn test_no_telemetry_sibling_keeps_feeder_meter() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let mixed_meter = builder.meter();
    builder.connect(grid_meter, mixed_meter);
    let feeder_meter = builder.meter();
    builder.connect(mixed_meter, feeder_meter);
    let pv_no_telemetry = builder.add_component_with_mode(
        ComponentCategory::Inverter(InverterType::Pv),
        OperationalMode::ControlOnly,
    );
    builder.connect(mixed_meter, pv_no_telemetry);
    builder.connect(feeder_meter, pv_no_telemetry);
    let load_meter = builder.meter();
    builder.connect(grid_meter, load_meter);

    let graph = builder.build(None)?;
    // The feeder meter (#3) stays in the fallback sum: dropping it would
    // leave the line it measures with no source at all.
    let expr = aggregate(
        &graph,
        BTreeSet::from([mixed_meter.component_id()]),
        SourcePreference::MetersFirst { by_config: false },
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3)");
    Ok(())
}

/// A no-telemetry meter sibling has no reading to subtract, so its parent
/// cannot form a substitution or subtraction group: the reporting targets
/// are measured directly instead.
///
/// Topology (ids): `Grid:0 → GridMeter:1 → PVMeter:2 → {PV:3, Meter:4 (no
/// telemetry) → PV:5}`.
#[test]
fn test_no_telemetry_meter_sibling_blocks_group() -> Result<(), Error> {
    let mut builder = ComponentGraphBuilder::new();
    let grid = builder.grid();
    let grid_meter = builder.meter();
    builder.connect(grid, grid_meter);
    let pv_meter = builder.meter();
    builder.connect(grid_meter, pv_meter);
    let pv = builder.solar_inverter();
    builder.connect(pv_meter, pv);
    let silent =
        builder.add_component_with_mode(ComponentCategory::Meter, OperationalMode::Inactive);
    builder.connect(pv_meter, silent);
    let pv_below = builder.solar_inverter();
    builder.connect(silent, pv_below);

    let graph = builder.build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, 0.0) + COALESCE(#5, 0.0)"
    );
    Ok(())
}
