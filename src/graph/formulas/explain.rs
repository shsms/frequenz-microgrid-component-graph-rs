// License: MIT
// Copyright © 2026 Frequenz Energy-as-a-Service GmbH

//! Structured explanations for generated formulas.
//!
//! Every formula generator can also report *why* each part of its formula is
//! there. The result is an [`ExplainedFormula`]: the usual [`Formula`] plus a
//! tree of [`Explanation`] nodes. Each node names its role (the
//! [`ExplanationKind`]), gives the reason in plain words, lists the components
//! it covers, and shows its own sub-expression as text.
//!
//! The explanation tree follows how the formula was *built*, not the final
//! simplified string. The final string may merge or drop parts (for example,
//! nested `COALESCE` calls are flattened). Use `component_ids` to link an
//! explanation to the graph, and `rendered` to find its text in the full
//! formula on a best-effort basis.
//!
//! Inside the crate, each node also keeps the sub-expression it explains
//! ([`Explanation::expr`]). [`ExplainedFormula::to_commented_string`] uses it
//! to render the formula with the reasons as `//` comments, aligned exactly.

// The commented renderer backs `to_commented_string`, which only the
// `explain` API exposes.
#[cfg(feature = "explain")]
mod comment;

use super::expr::Expr;
use super::formula::Formula;

/// The role a formula part plays. See each variant's docs for the shape it
/// describes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[non_exhaustive]
pub enum ExplanationKind {
    /// A device measured by its own reading, with a `0.0` fallback so the
    /// term still resolves when the device is offline.
    DirectReading,
    /// A component that provides no telemetry: it has no reading. In an
    /// aggregating formula it contributes `0.0`; in a non-aggregating
    /// (coalesce) formula it emits no value; as a dropped part it emits
    /// nothing at all.
    NoTelemetryZero,
    /// A meter measured by its bare reading, with no fallback.
    BareMeter,
    /// A grid meter measured by its bare reading. It carries the site's
    /// unmodeled consumer load, which no sum of its children accounts for,
    /// so it has no fallback.
    BareGridMeter,
    /// A meter's own reading, backed by the best-effort sum of its children
    /// in case the reading goes missing.
    MeterWithChildBackup,
    /// A meter that provides no reading of its own: it is measured by the
    /// best-effort sum of its children instead.
    NoTelemetryMeterDrill,
    /// A fallback ladder over a meter and its children: try each source in
    /// order and use the first one that has a value.
    MeterDrill {
        /// Whether the meter reading is the primary source (`true`) or the
        /// component readings are (`false`).
        prefers_meters: bool,
    },
    /// The exact sum of a group's readings (`#a + #b`). It is null unless
    /// every member reports, so a missing reading skips this rung instead of
    /// silently undercounting.
    ExactSum,
    /// A meter reading standing in for the group it measures.
    MeterReading,
    /// The best-effort sum of a group's readings (`COALESCE(#a, 0.0) + ...`):
    /// each reading or 0, so it always resolves.
    BestEffortSum,
    /// A sum of a group's readings where some term carries no fallback of
    /// its own (a bare meter reading, for one), so the sum goes missing with
    /// that reading. [`Self::BestEffortSum`] is the same sum where every
    /// term resolves; the two are distinct because only one of them can be
    /// relied on to produce a value.
    ChildrenSum,
    /// A child left out of a fallback sum because its flow is already
    /// counted elsewhere. Nothing is emitted.
    ChildSkipped {
        /// The contributing siblings this child feeds: their readings
        /// already carry its flow, so even a bare reading would count that
        /// line twice. Empty when the child was skipped for its outside
        /// feeds instead.
        feeds: Vec<u64>,
        /// The other feeds into this child from outside the parent meter (a
        /// parallel line): its reading holds more than the parent passes.
        /// Empty when the child was skipped for feeding a sibling instead.
        fed_from: Vec<u64>,
    },
    /// A component group fed through several parallel meters. Each meter
    /// measures a distinct feed line, so the meter readings sum to the
    /// group's throughput.
    Diamond,
    /// A component group measured as its parent meter(s) minus their other
    /// children, with the group's own readings as fallback.
    Subtraction,
    /// The parent meter(s) minus the sibling readings: everything through
    /// the parents except what the siblings account for, which is exactly
    /// the group. The siblings are subtracted by their bare readings — a
    /// `COALESCE(_, 0.0)` there would attribute a missing sibling's power
    /// to the group.
    MeterDifference {
        /// The summed parent meters the siblings are subtracted from.
        meters: Vec<u64>,
        /// The sibling components whose readings are subtracted.
        subtracted: Vec<u64>,
    },
    /// Fallbacks are disabled by config: each target is measured by its own
    /// bare reading only.
    FallbacksDisabled,
    /// A plain sum of independent measurement terms.
    TermSum,
    /// `MIN(x, 0.0)`: producers feed power in, which is negative by the
    /// passive sign convention. The clamp discards any consumption measured
    /// on the same lines.
    ProducerClamp,
    /// `MAX(x, 0.0)`: consumption cannot be negative, so the clamp discards
    /// any production measured on the same lines.
    ConsumerClamp,
    /// A meter's reading minus its modeled successors: the load connected to
    /// the meter that is not in the graph (the "phantom load").
    PhantomLoadResidual,
    /// A modeled successor whose reading is subtracted from its meter, so
    /// only the meter's unmodeled share remains.
    SubtractedSuccessor,
    /// A last-resort `0.0`: it keeps the term total (always resolving) when
    /// every real source is missing.
    DefaultZero,
    /// The grid formula minus the non-consumer groups (producers, storage):
    /// what remains is the site's consumption.
    NonConsumerSubtraction,
    /// A storage component left out of a consumption sum: what a battery
    /// draws is storage flow, not site consumption, so the component adds
    /// no term even when it reports.
    StorageNotConsumption,
    /// A `COALESCE` chain for non-aggregating metrics (voltage, frequency):
    /// the first source that has a value wins, in preference order.
    CoalesceChain,
    /// The root of a metric's explanation tree.
    Metric {
        /// The metric name, e.g. `"grid"` or `"producer"`.
        metric: String,
    },
}

/// One node of a formula's explanation tree: what role this part plays, why
/// it is there, and which components it covers.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct Explanation {
    /// The role this part plays (machine-readable).
    pub kind: ExplanationKind,
    /// Why this part is there, in plain words.
    pub rationale: String,
    /// The components this part measures or covers.
    pub component_ids: Vec<u64>,
    /// The sub-expression this part explains ([`Expr::None`] when it emits
    /// nothing). The commented renderer matches it against the parent
    /// expression's operands, so the reasons stay aligned with the formula
    /// even when the expression algebra reshapes the tree.
    ///
    /// Serialized as `rendered`, the text of this expression — see
    /// [`Self::rendered`].
    #[cfg_attr(
        feature = "serde",
        serde(rename = "rendered", serialize_with = "serialize_rendered")
    )]
    pub(crate) expr: Expr,
    /// The parts this one is built from.
    pub children: Vec<Explanation>,
    /// The plural phrasing for a run of same-shaped sibling parts, with
    /// `{n}` standing in for the number of parts the comment folded ("Each
    /// of the {n} PV inverters …") — totalled across an expanded run's
    /// members, so leaves count once per group member. The commented
    /// renderer folds such a run under one comment built from these.
    /// `None` for parts that never form runs.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) group_rationale: Option<String>,
    /// The one-line reason a member keeps when a run of same-shaped siblings
    /// folds expanded (each member laid out in full, the shared reasons
    /// printed once above the run): its identity — which meter, which group —
    /// with the shared mechanism left to the run's comment. `None` for parts
    /// that never fold expanded.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) member_rationale: Option<String>,
}

/// Serializes an explanation's expression as its rendered text, so the wire
/// shape carries the text rather than the expression tree.
#[cfg(feature = "serde")]
fn serialize_rendered<S: serde::Serializer>(expr: &Expr, serializer: S) -> Result<S::Ok, S::Error> {
    match expr {
        Expr::None => serializer.serialize_none(),
        expr => serializer.serialize_some(&expr.to_string()),
    }
}

impl Explanation {
    /// This part's own sub-expression as text. `None` when the part emits
    /// nothing (for example a skipped child). The final formula may merge
    /// this text with its neighbours, so matching it there is best-effort.
    ///
    /// Rendered on demand: an explanation tree is often built and discarded
    /// (every plain `*_formula` builds one), so the text is not worth keeping
    /// on every node.
    // Only the `explain` API exposes this; without it, the tests below are
    // the sole callers.
    #[cfg_attr(not(feature = "explain"), allow(dead_code))]
    pub fn rendered(&self) -> Option<String> {
        match &self.expr {
            Expr::None => None,
            expr => Some(expr.to_string()),
        }
    }

    /// Creates an explanation for `expr`, collecting `component_ids` from the
    /// expression and all `children`.
    pub(crate) fn new(
        kind: ExplanationKind,
        rationale: impl Into<String>,
        expr: &Expr,
        children: Vec<Explanation>,
    ) -> Self {
        let mut component_ids = expr.component_ids();
        for child in &children {
            component_ids.extend(child.component_ids.iter().copied());
        }
        component_ids.sort_unstable();
        component_ids.dedup();
        Explanation {
            kind,
            rationale: rationale.into(),
            component_ids,
            children,
            expr: expr.clone(),
            group_rationale: None,
            member_rationale: None,
        }
    }

    /// An explanation that emits no expression of its own, covering the given
    /// components (for example a skipped child).
    pub(crate) fn silent(
        kind: ExplanationKind,
        rationale: impl Into<String>,
        component_ids: Vec<u64>,
    ) -> Self {
        Explanation {
            kind,
            rationale: rationale.into(),
            component_ids,
            children: Vec::new(),
            expr: Expr::None,
            group_rationale: None,
            member_rationale: None,
        }
    }

    /// Sets the plural phrasing the commented renderer uses when a run of
    /// same-shaped siblings folds under one comment; `{n}` stands in for
    /// the member count.
    pub(crate) fn each(mut self, group_rationale: impl Into<String>) -> Self {
        self.group_rationale = Some(group_rationale.into());
        self
    }

    /// Like [`Explanation::silent`], with `parts` as children: a part that
    /// emits no expression but still records its own silent parts (for
    /// example a no-telemetry grid meter and the children it leaves out).
    /// The parts' component ids are collected into `component_ids`.
    pub(crate) fn silent_with_parts(
        kind: ExplanationKind,
        rationale: impl Into<String>,
        component_ids: Vec<u64>,
        parts: Vec<Explanation>,
    ) -> Self {
        let mut silent = Explanation::new(kind, rationale, &Expr::None, parts);
        silent.component_ids.extend(component_ids);
        silent.component_ids.sort_unstable();
        silent.component_ids.dedup();
        silent
    }

    /// Every node of the explanation tree in pre-order, so a test can search
    /// it with the ordinary iterator methods rather than its own recursion.
    #[cfg(test)]
    pub(crate) fn nodes(&self) -> Vec<&Explanation> {
        let mut nodes = vec![self];
        for child in &self.children {
            nodes.extend(child.nodes());
        }
        nodes
    }
}

/// An expression together with the explanation of how it was built. The
/// internal carrier every formula generator threads through.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Explained {
    pub(crate) expr: Expr,
    pub(crate) explanation: Explanation,
}

impl Explained {
    /// A leaf: an expression explained in one node, with no children.
    pub(crate) fn leaf(expr: Expr, kind: ExplanationKind, rationale: impl Into<String>) -> Self {
        let explanation = Explanation::new(kind, rationale, &expr, Vec::new());
        Explained { expr, explanation }
    }

    /// Sets the plural phrasing for folded runs on the explanation; see
    /// [`Explanation::each`].
    pub(crate) fn each(mut self, group_rationale: impl Into<String>) -> Self {
        self.explanation.group_rationale = Some(group_rationale.into());
        self
    }

    /// Sets the member reason for expanded folded runs on the explanation;
    /// see [`Explanation::member_rationale`].
    pub(crate) fn member(mut self, member_rationale: impl Into<String>) -> Self {
        self.explanation.member_rationale = Some(member_rationale.into());
        self
    }

    /// Adds components the explanation covers beyond those its expression
    /// names: for a part that stands for a component without naming it (a
    /// no-telemetry `0.0`, say), so the component still shows up in
    /// [`Explanation::component_ids`].
    pub(crate) fn covering(mut self, component_ids: impl IntoIterator<Item = u64>) -> Self {
        self.explanation.component_ids.extend(component_ids);
        self.explanation.component_ids.sort_unstable();
        self.explanation.component_ids.dedup();
        self
    }

    /// A part that emits no expression (its expression is [`Expr::None`],
    /// which vanishes from any sum): it records a component that is left
    /// out on purpose, and why.
    pub(crate) fn silent(
        kind: ExplanationKind,
        rationale: impl Into<String>,
        component_ids: Vec<u64>,
    ) -> Self {
        Explained {
            expr: Expr::None,
            explanation: Explanation::silent(kind, rationale, component_ids),
        }
    }

    /// Explains `expr` as built from `parts` (whose explanations become the
    /// children). `expr` must be the caller's combination of the parts'
    /// expressions.
    pub(crate) fn compose(
        expr: Expr,
        kind: ExplanationKind,
        rationale: impl Into<String>,
        parts: Vec<Explanation>,
    ) -> Self {
        let explanation = Explanation::new(kind, rationale, &expr, parts);
        Explained { expr, explanation }
    }

    /// Wraps this term's expression with `wrap`, adding one explanation node
    /// on top (for example a clamp).
    pub(crate) fn wrap(
        self,
        wrap: impl FnOnce(Expr) -> Expr,
        kind: ExplanationKind,
        rationale: impl Into<String>,
    ) -> Self {
        let expr = wrap(self.expr);
        let explanation = Explanation::new(kind, rationale, &expr, vec![self.explanation]);
        Explained { expr, explanation }
    }

    /// Converts this term into an [`ExplainedFormula`], adding the metric
    /// root node on top.
    pub(crate) fn into_formula(
        self,
        metric: &str,
        rationale: impl Into<String>,
    ) -> ExplainedFormula {
        let explanation = Explanation::new(
            ExplanationKind::Metric {
                metric: metric.to_string(),
            },
            rationale,
            &self.expr,
            vec![self.explanation],
        );
        ExplainedFormula {
            formula: Formula::new(self.expr),
            explanation,
        }
    }
}

/// Renders the expression, like [`Expr`]'s `Display`.
impl std::fmt::Display for Explained {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.expr.fmt(f)
    }
}

/// Sums the terms' expressions and explains the sum as one node with the
/// terms as children. `None` if there are no terms. A single term keeps its
/// own explanation without an extra sum node.
pub(crate) fn sum_explained(
    terms: impl IntoIterator<Item = Explained>,
    kind: ExplanationKind,
    rationale: impl Into<String>,
) -> Option<Explained> {
    let mut terms = terms.into_iter();
    let first = terms.next()?;
    let Some(second) = terms.next() else {
        return Some(first);
    };
    let mut expr = first.expr + second.expr;
    let mut children = vec![first.explanation, second.explanation];
    for term in terms {
        expr = expr + term.expr;
        children.push(term.explanation);
    }
    Some(Explained::compose(expr, kind, rationale, children))
}

/// Formats component ids as `#a, #b, ...` for rationale texts.
pub(crate) fn id_list(ids: &[u64]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Uppercases a label's first character, for a label starting a sentence:
/// "battery inverter" becomes "Battery inverter" ("PV inverter" stays).
pub(crate) fn capitalized(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Formats a count with a naively pluralized label: "1 CHP", "3 PV meters".
pub(crate) fn pluralized(count: usize, label: &str) -> String {
    match count {
        1 => format!("1 {label}"),
        _ => format!("{count} {label}s"),
    }
}

/// A formula together with the tree of explanations for its parts.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct ExplainedFormula {
    /// The formula, identical to the one from the plain `*_formula` method.
    pub formula: Formula,
    /// The explanation tree. Its root covers the whole formula.
    pub explanation: Explanation,
}

/// Renders the plain formula, without comments. Same string as [`Formula`]'s
/// `Display`. Use [`to_commented_string`](ExplainedFormula::to_commented_string)
/// for the commented outline.
impl std::fmt::Display for ExplainedFormula {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.formula.fmt(f)
    }
}

/// The final formula as a tree, for rendering and highlighting in UIs.
///
/// This mirrors the formula string exactly: rendering this tree with the
/// formula grammar gives the same string as [`Formula`]'s `Display`.
#[cfg(feature = "explain")]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(tag = "op", rename_all = "snake_case")
)]
#[non_exhaustive]
pub enum FormulaAst {
    /// An empty formula, which evaluates to no value.
    None,
    /// A negation of an expression.
    Neg {
        /// The negated expression.
        param: Box<FormulaAst>,
    },
    /// A numeric constant.
    Number {
        /// The constant value.
        value: f64,
    },
    /// A reference to a component's reading (`#id`).
    Component {
        /// The component id.
        component_id: u64,
    },
    /// An addition of expressions.
    Add {
        /// The summands.
        params: Vec<FormulaAst>,
    },
    /// A subtraction: the first expression minus all the others.
    Sub {
        /// The first operand and the subtracted ones.
        params: Vec<FormulaAst>,
    },
    /// A `COALESCE`: the first expression that has a value.
    Coalesce {
        /// The alternatives, in order.
        params: Vec<FormulaAst>,
    },
    /// A `MIN` over expressions.
    Min {
        /// The compared expressions.
        params: Vec<FormulaAst>,
    },
    /// A `MAX` over expressions.
    Max {
        /// The compared expressions.
        params: Vec<FormulaAst>,
    },
}

#[cfg(feature = "explain")]
impl From<&Expr> for FormulaAst {
    fn from(expr: &Expr) -> Self {
        let convert = |params: &[Expr]| params.iter().map(FormulaAst::from).collect();
        match expr {
            Expr::None => FormulaAst::None,
            Expr::Neg { param } => FormulaAst::Neg {
                param: Box::new(FormulaAst::from(param.as_ref())),
            },
            Expr::Number { value } => FormulaAst::Number { value: *value },
            Expr::Component { component_id } => FormulaAst::Component {
                component_id: *component_id,
            },
            Expr::Add { params } => FormulaAst::Add {
                params: convert(params),
            },
            Expr::Sub { params } => FormulaAst::Sub {
                params: convert(params),
            },
            Expr::Coalesce { params } => FormulaAst::Coalesce {
                params: convert(params),
            },
            Expr::Min { params } => FormulaAst::Min {
                params: convert(params),
            },
            Expr::Max { params } => FormulaAst::Max {
                params: convert(params),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explanation_collects_component_ids() {
        let expr = Expr::component(2).coalesce(Expr::number(0.0));
        let child = Explanation::silent(
            ExplanationKind::ChildSkipped {
                feeds: vec![],
                fed_from: vec![],
            },
            "skipped",
            vec![7],
        );
        let explanation = Explanation::new(
            ExplanationKind::DirectReading,
            "reading or 0",
            &expr,
            vec![child],
        );
        assert_eq!(explanation.component_ids, vec![2, 7]);
        assert_eq!(explanation.rendered().as_deref(), Some("COALESCE(#2, 0.0)"));
    }

    #[test]
    fn test_silent_explanation_has_no_rendering() {
        let explanation = Explanation::silent(
            ExplanationKind::ChildSkipped {
                feeds: vec![],
                fed_from: vec![],
            },
            "skipped",
            vec![3],
        );
        assert_eq!(explanation.rendered(), None);
        assert_eq!(explanation.component_ids, vec![3]);
    }

    #[test]
    fn test_sum_explained_single_term_keeps_explanation() {
        let term = Explained::leaf(
            Expr::component(1),
            ExplanationKind::MeterReading,
            "the meter",
        );
        let summed = sum_explained([term.clone()], ExplanationKind::TermSum, "sum").unwrap();
        assert_eq!(summed, term);
    }

    #[test]
    fn test_sum_explained_combines_terms() {
        let a = Explained::leaf(Expr::component(1), ExplanationKind::MeterReading, "a");
        let b = Explained::leaf(Expr::component(2), ExplanationKind::MeterReading, "b");
        let summed = sum_explained([a, b], ExplanationKind::TermSum, "sum").unwrap();
        assert_eq!(summed.expr.to_string(), "#1 + #2");
        assert_eq!(summed.explanation.children.len(), 2);
        assert_eq!(summed.explanation.component_ids, vec![1, 2]);
    }

    /// The public explanation types stay serializable under the `serde`
    /// feature.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serializable() {
        fn assert_serialize<T: serde::Serialize>() {}
        assert_serialize::<ExplainedFormula>();
        assert_serialize::<Explanation>();
        assert_serialize::<ExplanationKind>();
        #[cfg(feature = "explain")]
        assert_serialize::<FormulaAst>();
        assert_serialize::<Formula>();
    }

    /// The serialized shape of the field-carrying kinds, the AST, and the
    /// formula string is pinned, so a field rename cannot ship unnoticed.
    /// A node's wire shape, which downstream renderers match against the
    /// formula text: `rendered` carries the part's expression as a string,
    /// and is `null` for a part that emits nothing. Pinned because consumers
    /// key off it, and it is derived from `expr` rather than stored.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serialized_explanation_shape() {
        use serde_json::{json, to_value};

        let explanation = Explanation::new(
            ExplanationKind::DirectReading,
            "why",
            &Expr::component(2).coalesce(Expr::number(0.0)),
            vec![Explanation::silent(
                ExplanationKind::ChildSkipped {
                    feeds: vec![],
                    fed_from: vec![],
                },
                "left out",
                vec![7],
            )],
        );
        assert_eq!(
            to_value(&explanation).unwrap(),
            json!({
                "kind": {"type": "direct_reading"},
                "rationale": "why",
                "component_ids": [2, 7],
                "rendered": "COALESCE(#2, 0.0)",
                "children": [{
                    "kind": {"type": "child_skipped", "feeds": [], "fed_from": []},
                    "rationale": "left out",
                    "component_ids": [7],
                    "rendered": null,
                    "children": [],
                }],
            })
        );
    }

    /// [`ExplainedFormula`]'s own envelope is pinned too: `formula` carries
    /// the formula string, `explanation` the tree.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serialized_explained_formula_shape() {
        use serde_json::{json, to_value};

        let explained = Explained::leaf(Expr::component(2), ExplanationKind::DirectReading, "why")
            .into_formula("component", "what");
        assert_eq!(
            to_value(&explained).unwrap(),
            json!({
                "formula": "#2",
                "explanation": {
                    "kind": {"type": "metric", "metric": "component"},
                    "rationale": "what",
                    "component_ids": [2],
                    "rendered": "#2",
                    "children": [{
                        "kind": {"type": "direct_reading"},
                        "rationale": "why",
                        "component_ids": [2],
                        "rendered": "#2",
                        "children": [],
                    }],
                },
            })
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialized_shape() {
        use serde_json::{json, to_value};

        assert_eq!(
            to_value(ExplanationKind::ChildSkipped {
                feeds: vec![3],
                fed_from: vec![],
            })
            .unwrap(),
            json!({"type": "child_skipped", "feeds": [3], "fed_from": []})
        );
        assert_eq!(
            to_value(ExplanationKind::MeterDifference {
                meters: vec![1],
                subtracted: vec![4],
            })
            .unwrap(),
            json!({"type": "meter_difference", "meters": [1], "subtracted": [4]})
        );
        assert_eq!(
            to_value(ExplanationKind::MeterDrill {
                prefers_meters: true,
            })
            .unwrap(),
            json!({"type": "meter_drill", "prefers_meters": true})
        );
        assert_eq!(
            to_value(ExplanationKind::Metric {
                metric: "grid".to_string(),
            })
            .unwrap(),
            json!({"type": "metric", "metric": "grid"})
        );
        #[cfg(feature = "explain")]
        assert_eq!(
            to_value(FormulaAst::from(
                &Expr::component(1).coalesce(Expr::number(0.0))
            ))
            .unwrap(),
            json!({"op": "coalesce", "params": [
                {"op": "component", "component_id": 1},
                {"op": "number", "value": 0.0},
            ]})
        );
        assert_eq!(
            to_value(Formula::new(Expr::component(1))).unwrap(),
            json!("#1")
        );
    }

    #[cfg(feature = "explain")]
    #[test]
    fn test_formula_ast_mirrors_expr() {
        let expr = Expr::component(1)
            .coalesce(Expr::component(2) + Expr::component(3))
            .min(Expr::number(0.0));
        let ast = FormulaAst::from(&expr);
        assert_eq!(
            ast,
            FormulaAst::Min {
                params: vec![
                    FormulaAst::Coalesce {
                        params: vec![
                            FormulaAst::Component { component_id: 1 },
                            FormulaAst::Add {
                                params: vec![
                                    FormulaAst::Component { component_id: 2 },
                                    FormulaAst::Component { component_id: 3 },
                                ]
                            },
                        ]
                    },
                    FormulaAst::Number { value: 0.0 },
                ]
            }
        );
    }
}
