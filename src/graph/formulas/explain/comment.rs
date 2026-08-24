// License: MIT
// Copyright © 2026 Frequenz Energy-as-a-Service GmbH

//! Renders an [`ExplainedFormula`] as a formula string with `//` comments.
//!
//! The renderer walks the explanation tree alone. Each node carries the
//! sub-expression it explains ([`Explanation::expr`]), so the reasons and the
//! formula structure are matched structurally, not by comparing strings. Where
//! the expression algebra flattened a part into its parent (the `+`/`-`
//! constructors splice nested sums; `coalesce`/`min`/`max` merge same-kind
//! calls), [`cover_at`] matches the part to the run of operands it became, so
//! its reasons are kept.

use super::{ExplainedFormula, Explanation, ExplanationKind};
use crate::graph::formulas::expr::Expr;

/// One indentation step.
const INDENT: &str = "    ";

impl ExplainedFormula {
    /// Renders the formula as a valid formula string with `//` comments.
    ///
    /// The result is the same formula as [`Formula`]'s `Display`, but laid out
    /// over several lines: a `COALESCE`, `MIN` or `MAX` call — or a `+` / `-`
    /// sum — whose parts have reasons of their own is broken open, one
    /// operand per line, with the reason on a `//` comment line just above
    /// it; an operand with no reason of its own (like a clamp's `0.0`) stays
    /// in place, uncommented.
    /// A part that emits no expression (a skipped child, a no-telemetry
    /// component) shows only its comment. Simple parts — a bare reading, a sum
    /// with a single reason — stay on one line.
    ///
    /// A run of two or more same-shaped operands (one term per component,
    /// with reasons differing only in the component) folds under a single
    /// comment describing every member ("Each of the 12 PV inverters …"),
    /// and each member stays on one line. A run of same-shaped meter groups
    /// folds expanded: the shared reasons print once above the run — even
    /// when the groups' child counts differ — and each group keeps a
    /// one-line identity comment over its fully laid-out term. Long reasons
    /// wrap at a fixed width.
    ///
    /// Dropping the `//` comment lines and the layout whitespace recovers
    /// [`Formula`]'s `Display` string exactly, so a formula parser that skips
    /// `//` comments reads the same formula. A formula that is `None` prints
    /// its reasons and the body `None`.
    ///
    /// Real output for a battery meter (#1) over one battery inverter (#2),
    /// with meters preferred by config:
    ///
    /// ```text
    /// // battery: The total battery power. Batteries are DC components with no AC reading of their own, so
    /// // they are measured through their inverters (and battery meters).
    /// // Battery meter #1 measures exactly this group (#2). Meters are preferred by config, so its reading
    /// // comes first; its child is the fallback.
    /// COALESCE(
    ///     // The meter's own reading is the primary source: it measures all of its children together.
    ///     #1,
    ///     // Child battery inverter #2's reading. The 0.0 fallback keeps the term total when the device is
    ///     // offline.
    ///     #2,
    ///     0.0
    /// )
    /// ```
    ///
    /// [`Formula`]: crate::Formula
    #[must_use]
    pub fn to_commented_string(&self) -> String {
        let mut lines = Vec::new();
        if render(&self.explanation, 0, &mut lines, Comments::All).is_none() {
            // The whole formula is None (it has no source at all): print the
            // body `Display` prints, so stripping the comments still
            // recovers the plain formula string.
            lines.push(String::from("None"));
        }
        lines.join("\n")
    }
}

/// How to lay a broken-open expression out: as a named call with one operand
/// per line, or as a sum with the joining sign trailing the operand before it.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Call(&'static str),
    Sum(char),
}

/// One slot in a broken-open expression's layout, in reading order.
#[derive(Clone)]
enum Slot<'a> {
    /// An operand (or a spliced run of operands) explained by this child.
    Child(&'a Explanation),
    /// A comment-only slot, in place: a silent part, or a merged call's
    /// header — a child whose call wrapper vanished into the parent's, so
    /// the parent lays out its operands.
    Note(&'a Explanation),
    /// An operand rendered as bare text: one with no child of its own (like
    /// a clamp's `0.0`), or one kept as-is because the children below it do
    /// not line up with it.
    Verbatim(&'a Expr),
    /// A folded run's shared comments, printed once above its members.
    GroupComment(Vec<String>),
    /// One member of an expanded folded run: laid out in full like a
    /// [`Slot::Child`], but with only its one-line member reason above it —
    /// the shared reasons are in the run's [`Slot::GroupComment`].
    Expanded(&'a Explanation),
}

/// Which reasons [`render`] prints for a node.
#[derive(Clone, Copy, PartialEq)]
enum Comments {
    /// Every reason.
    All,
    /// Only the parts' reasons: the node's own is already on the page (the
    /// same text was printed for an earlier sibling at the same level).
    SkipOwn,
    /// No reasons at all: the node is a member of an expanded folded run,
    /// whose reasons are all in the run's group comment.
    Quiet,
}

/// Appends the commented, indented lines for `node` at `depth`.
///
/// Returns the index (in `lines`) of the expression's final line, so a caller
/// can append a separator there. It is the last expression line, not simply
/// the last pushed line — a silent part's comment may follow it. `None` when
/// `node` itself is silent.
///
/// `comments` says which reasons to print: [`Comments::SkipOwn`] when the
/// node's own reason is already on the page, [`Comments::Quiet`] inside an
/// expanded folded run, where the whole subtree's reasons are already in the
/// run's group comment.
fn render(
    node: &Explanation,
    depth: usize,
    lines: &mut Vec<String>,
    comments: Comments,
) -> Option<usize> {
    if comments == Comments::All {
        push_comment(node, depth, lines);
    }
    // `SkipOwn` covers the node's own reason only; `Quiet` the whole subtree.
    let parts = match comments {
        Comments::Quiet => Comments::Quiet,
        _ => Comments::All,
    };
    if node.expr == Expr::None {
        // A silent part: comments only. Its parts (a None coalesce chain's
        // skipped sources, for example) are silent too, so their reasons
        // still get their comment lines.
        for child in &node.children {
            render(child, depth, lines, parts);
        }
        return None;
    }

    // A node whose only expression-emitting child holds the same expression
    // adds no text of its own — it just annotates that child (the metric root
    // over the whole formula, or a sum collapsed to its one real term). Emit
    // the parts in order at the same depth, so nothing is printed twice.
    let mut emitting = node.children.iter().filter(|c| c.expr != Expr::None);
    if let (Some(child), None) = (emitting.next(), emitting.next())
        && child.expr == node.expr
    {
        let mut end = None;
        for part in &node.children {
            // A silent part renders its comments and returns `None`, so the
            // last expression line stands.
            end = render(part, depth, lines, parts).or(end);
        }
        return end;
    }

    // Break the expression open when the node's children account for its
    // operands. Operands with no child (like a clamp's 0.0) stay in place.
    let layout = match &node.expr {
        Expr::Coalesce { params } => Some((Layout::Call("COALESCE"), params)),
        Expr::Min { params } => Some((Layout::Call("MIN"), params)),
        Expr::Max { params } => Some((Layout::Call("MAX"), params)),
        Expr::Add { params } => Some((Layout::Sum('+'), params)),
        Expr::Sub { params } => Some((Layout::Sum('-'), params)),
        // A bare reading or a number: one line.
        _ => None,
    };
    if let Some((layout, params)) = layout
        && let Some(slots) = plan(node, params, false)
    {
        // Nothing to fold in a quiet subtree: there are no comments to share.
        let slots = match parts {
            Comments::Quiet => slots,
            _ => fold_runs(slots),
        };
        return Some(emit(layout, &slots, depth, lines, parts));
    }

    // One line. A silent child adds no text to it, so its reason joins the
    // comment block above the line — below it, it would read as the next
    // operand's reason.
    for child in node.children.iter().filter(|c| c.expr == Expr::None) {
        render(child, depth, lines, parts);
    }
    lines.push(format!("{}{}", INDENT.repeat(depth), node.expr));
    Some(lines.len() - 1)
}

/// Matches `node`'s children to its expression's operands, in order.
///
/// Each expression-emitting child covers the operand equal to its own
/// expression, or the run of operands it was flattened into; operands passed
/// over on the way (constants like a clamp's `0.0`) are kept verbatim. Silent
/// children keep their place as comment-only notes. `None` when the children
/// do not line up with the operands, or — unless `lenient` — when no child
/// emits an expression, since breaking the expression open would then add
/// nothing. The spliced-call recursion passes `lenient` because there the
/// expression is already broken open.
fn plan<'a>(node: &'a Explanation, params: &'a [Expr], lenient: bool) -> Option<Vec<Slot<'a>>> {
    let mut slots = Vec::new();
    let mut next = 0;
    let mut any_child = false;
    for child in &node.children {
        if child.expr == Expr::None {
            slots.push(Slot::Note(child));
            continue;
        }
        let mut covered = None;
        while covered.is_none() && next < params.len() {
            covered = cover_at(&node.expr, &child.expr, &params[next..], next);
            if covered.is_none() {
                slots.push(Slot::Verbatim(&params[next]));
                next += 1;
            }
        }
        match covered? {
            // The child lays its own operands out exactly like the run
            // reads in the parent, so it renders as itself.
            Cover::Run(len) => {
                slots.push(Slot::Child(child));
                next += len;
            }
            Cover::CallRun(len) => {
                // The child's own call was merged into the parent's operand
                // list, so its `NAME(` wrapper is gone from the final text.
                // Keep its comment in place and lay its operands out like
                // the parent's own.
                slots.push(Slot::Note(child));
                let run = &params[next..next + len];
                match plan(child, run, true) {
                    Some(inner) => slots.extend(inner),
                    // The grandchildren don't line up: keep the operands.
                    None => slots.extend(run.iter().map(Slot::Verbatim)),
                }
                next += len;
            }
        }
        any_child = true;
    }
    slots.extend(params[next..].iter().map(Slot::Verbatim));
    (any_child || lenient).then_some(slots)
}

/// The comments a member subtree would print if broken open, in pre-order:
/// one entry per node. `None` when the subtree contains a silent part — its
/// comment has no operand to stand above in an inline rendering, so the
/// member must stay expanded.
fn comment_stack(node: &Explanation) -> Option<Vec<&Explanation>> {
    if node.expr == Expr::None {
        return None;
    }
    let mut stack = vec![node];
    for child in &node.children {
        stack.append(&mut comment_stack(child)?);
    }
    Some(stack)
}

/// Whether two nodes share a comment: the same kind and either the very same
/// rationale (an id-free shared reason, printed once) or the same plural
/// phrasing (set by the build site, printed with the fold's node count).
fn folds_with(x: &Explanation, y: &Explanation) -> bool {
    x.kind == y.kind
        && (x.rationale == y.rationale
            || (x.group_rationale.is_some() && x.group_rationale == y.group_rationale))
}

/// One position of a member's spine: a representative node, how many stack
/// entries collapsed into it, and whether they all share one rationale.
struct SpinePosition<'a> {
    node: &'a Explanation,
    count: usize,
    uniform: bool,
}

/// The comment stack compressed: consecutive entries that share a comment
/// (same kind, same rationale or plural phrasing) collapse into one
/// position. Two members with matching spines are measured the same way,
/// even when they collapse different member counts — a 4-child and a 6-child
/// meter group have the same spine.
fn spine<'a>(stack: &[&'a Explanation]) -> Vec<SpinePosition<'a>> {
    let mut out: Vec<SpinePosition<'a>> = Vec::new();
    for &node in stack {
        if let Some(last) = out.last_mut()
            && folds_with(last.node, node)
        {
            last.count += 1;
            last.uniform = last.uniform && last.node.rationale == node.rationale;
            continue;
        }
        out.push(SpinePosition {
            node,
            count: 1,
            uniform: true,
        });
    }
    out
}

/// Whether two members' spines fold into one shared comment stack: pairwise
/// nodes that share a comment.
fn foldable(a: &[SpinePosition<'_>], b: &[SpinePosition<'_>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| folds_with(x.node, y.node))
}

/// Replaces each run of two or more consecutive same-shaped
/// [`Slot::Child`]s with one [`Slot::GroupComment`] followed by its members:
/// the run's reasons are printed once — a rationale shared verbatim as-is,
/// an id-bearing one as its plural phrasing with `{n}` replaced by the
/// count of nodes it folded.
///
/// Members whose build site set a member reason fold expanded
/// ([`Slot::Expanded`]): each is laid out in full under its own one-line
/// identity, so groups of any size — even with differing child counts —
/// share their mechanism prose. Other members fold inline as plain
/// [`Slot::Verbatim`] operands, each on one line. Build sites set the member reason
/// together with the plural phrasing, so an expanded run's members all
/// carry one; a member without it would render with no comment at all.
fn fold_runs(slots: Vec<Slot<'_>>) -> Vec<Slot<'_>> {
    // A run needs two adjacent children; below that the spines — a subtree
    // walk each — could not be used.
    if slots
        .iter()
        .filter(|slot| matches!(slot, Slot::Child(_)))
        .count()
        < 2
    {
        return slots;
    }
    let spines: Vec<Option<Vec<SpinePosition<'_>>>> = slots
        .iter()
        .map(|slot| match slot {
            Slot::Child(child) => comment_stack(child).map(|stack| spine(&stack)),
            _ => None,
        })
        .collect();
    let mut out = Vec::with_capacity(slots.len());
    let mut start = 0;
    while start < slots.len() {
        let Some(first) = &spines[start] else {
            out.push(slots[start].clone());
            start += 1;
            continue;
        };
        let mut members: Vec<&[SpinePosition<'_>]> = vec![first.as_slice()];
        let mut end = start + 1;
        while end < slots.len()
            && let Some(next) = &spines[end]
            && foldable(first, next)
        {
            members.push(next.as_slice());
            end += 1;
        }
        if end - start < 2 {
            out.push(slots[start].clone());
            start = end;
            continue;
        }
        let comments = (0..first.len())
            .map(|position| {
                let base = members[0][position].node;
                let shared = members.iter().all(|member| {
                    member[position].uniform && member[position].node.rationale == base.rationale
                });
                match (shared, &base.group_rationale) {
                    (false, Some(plural)) => {
                        let total: usize = members.iter().map(|m| m[position].count).sum();
                        plural.replace("{n}", &total.to_string())
                    }
                    // Differing rationales can only have folded on the
                    // plural phrasing, so a missing one means shared.
                    _ => base.rationale.clone(),
                }
            })
            .collect();
        out.push(Slot::GroupComment(comments));
        let expanded = members
            .iter()
            .any(|member| member[0].node.member_rationale.is_some());
        out.extend(slots[start..end].iter().map(|slot| {
            let Slot::Child(child) = slot else {
                unreachable!("a run holds child slots")
            };
            match expanded {
                true => Slot::Expanded(child),
                // Inline on one line with no comment of its own, exactly
                // like any other verbatim operand.
                false => Slot::Verbatim(&child.expr),
            }
        }));
        start = end;
    }
    out
}

/// How a child covers operands of its parent's expression.
enum Cover {
    /// The run of operands the child renders as itself: one operand equal to
    /// its whole expression, or the run its `+`/`-` sum was flattened into.
    Run(usize),
    /// A run of operands the child's same-kind call (`COALESCE`/`MIN`/`MAX`)
    /// was merged into. The child's call wrapper is gone from the final
    /// text, so only its operands are laid out.
    CallRun(usize),
}

/// How `child` covers the operands at `rest` (the parent's operands from
/// `position` on): the first operand when it equals the child's whole
/// expression, or the run of operands the constructors flattened the child
/// into — `+`/`-` splice nested sums (a subtraction only into the front of
/// an enclosing `-`), and `coalesce`/`min`/`max` merge same-kind calls.
/// `None` when `child` covers no operands here.
fn cover_at(parent: &Expr, child: &Expr, rest: &[Expr], position: usize) -> Option<Cover> {
    if rest[0] == *child {
        return Some(Cover::Run(1));
    }
    let run = |spliced: &[Expr]| {
        (spliced.len() <= rest.len() && spliced[..] == rest[..spliced.len()])
            .then_some(spliced.len())
    };
    match (parent, child) {
        (Expr::Add { .. }, Expr::Add { params }) => run(params).map(Cover::Run),
        (Expr::Sub { .. }, Expr::Sub { params }) if position == 0 => run(params).map(Cover::Run),
        (Expr::Coalesce { .. }, Expr::Coalesce { params })
        | (Expr::Min { .. }, Expr::Min { params })
        | (Expr::Max { .. }, Expr::Max { params }) => run(params).map(Cover::CallRun),
        _ => None,
    }
}

/// Appends the lines of a broken-open expression: one operand per line —
/// children with their reasons, constants verbatim — notes as comment lines
/// in their place, and, for a [`Layout::Call`], the `NAME(` … `)` wrapper.
///
/// Returns the index of the expression's final line, like [`render`].
/// [`Comments::Quiet`] prints no reasons at all (an expanded fold member's
/// body); otherwise every slot's reasons are printed.
fn emit(
    layout: Layout,
    slots: &[Slot<'_>],
    depth: usize,
    lines: &mut Vec<String>,
    comments: Comments,
) -> usize {
    let quiet = comments == Comments::Quiet;
    let body_depth = match layout {
        Layout::Call(name) => {
            lines.push(format!("{}{name}(", INDENT.repeat(depth)));
            depth + 1
        }
        Layout::Sum(_) => depth,
    };
    let body_indent = INDENT.repeat(body_depth);
    let last = slots
        .iter()
        .rposition(|slot| !matches!(slot, Slot::Note(_) | Slot::GroupComment(_)));
    // A reason identical to one in the comment block directly above is not
    // repeated: a lone term after a folded run does not restate the reason
    // the run just printed. Only that previous block counts — an equal-text
    // group comment further on describes *different* members, so it always
    // prints (suppressing it would drop those members' only comments and
    // leave the earlier count claiming too few).
    let mut previous_block: Vec<String> = Vec::new();
    let mut first_operand = true;
    let mut end = 0;
    for (i, slot) in slots.iter().enumerate() {
        let expr = match slot {
            Slot::Note(child) => {
                if !quiet {
                    match child.expr {
                        // A silent part: render, not push_comment, so its own
                        // silent parts (if any) follow it.
                        Expr::None => {
                            render(child, body_depth, lines, Comments::All);
                        }
                        // A merged call's header note (the child emits its
                        // operands through the parent's layout): comment only.
                        _ => push_comment(child, body_depth, lines),
                    }
                    previous_block = vec![child.rationale.clone()];
                }
                continue;
            }
            Slot::GroupComment(shared) => {
                if !quiet {
                    for comment in shared {
                        push_comment_text(comment, body_depth, lines);
                    }
                    previous_block = shared.clone();
                }
                continue;
            }
            Slot::Child(child) | Slot::Expanded(child) => &child.expr,
            Slot::Verbatim(expr) => expr,
        };
        // A subtracted sum keeps the brackets `Display` gives it — without
        // them the flat string would read back as a longer chain.
        let bracket = layout == Layout::Sum('-') && !first_operand && expr.needs_brackets();
        first_operand = false;
        let mode = match slot {
            _ if quiet => Comments::Quiet,
            Slot::Child(child) => {
                let repeated = previous_block.contains(&child.rationale);
                if !repeated {
                    previous_block = vec![child.rationale.clone()];
                }
                // A suppressed reason keeps the block it repeats, so a chain
                // of equal-reason terms shares the one printed copy.
                match repeated {
                    true => Comments::SkipOwn,
                    false => Comments::All,
                }
            }
            // An expanded member keeps only its one-line member reason; the
            // rest of its subtree is quiet — its reasons are in the run's
            // group comment.
            Slot::Expanded(child) => {
                if let Some(member) = &child.member_rationale {
                    push_comment_text(member, body_depth, lines);
                    previous_block = vec![member.clone()];
                }
                Comments::Quiet
            }
            _ => Comments::All,
        };
        end = match (slot, bracket) {
            (Slot::Child(child) | Slot::Expanded(child), false) => {
                // A silent child pushes no expression line; the separator
                // target stays on the previous line.
                render(child, body_depth, lines, mode).unwrap_or(end)
            }
            (Slot::Child(child) | Slot::Expanded(child), true) => {
                lines.push(format!("{body_indent}("));
                render(child, body_depth + 1, lines, mode);
                lines.push(format!("{body_indent})"));
                lines.len() - 1
            }
            // A folded member renders inline, like a verbatim operand.
            (_, false) => {
                lines.push(format!("{body_indent}{expr}"));
                lines.len() - 1
            }
            (_, true) => {
                lines.push(format!("{body_indent}({expr})"));
                lines.len() - 1
            }
        };
        let trailing = match layout {
            // Between operands only: the flat string has no trailing comma,
            // and stripping the comments must recover it exactly.
            Layout::Call(_) if Some(i) != last => String::from(","),
            // The joining sign of the next operand trails this line.
            Layout::Sum(sign) if Some(i) != last => format!(" {sign}"),
            _ => String::new(),
        };
        lines[end].push_str(&trailing);
    }
    match layout {
        Layout::Call(_) => {
            lines.push(format!("{})", INDENT.repeat(depth)));
            lines.len() - 1
        }
        // `plan` guarantees at least one expression slot, so `end` is set.
        Layout::Sum(_) => end,
    }
}

/// Pushes `node`'s reason as `//` comment lines at `depth`. The metric root
/// leads its reason with the metric name.
fn push_comment(node: &Explanation, depth: usize, lines: &mut Vec<String>) {
    let reason = match &node.kind {
        ExplanationKind::Metric { metric } => format!("{metric}: {}", node.rationale),
        _ => node.rationale.clone(),
    };
    push_comment_text(&reason, depth, lines);
}

/// The total line width comments are wrapped to (indent and `// ` included).
const COMMENT_WIDTH: usize = 100;

/// Pushes `text` as `//` comment lines at `depth`, wrapping at
/// [`COMMENT_WIDTH`] on word boundaries.
fn push_comment_text(text: &str, depth: usize, lines: &mut Vec<String>) {
    let indent = INDENT.repeat(depth);
    let width = COMMENT_WIDTH
        .saturating_sub(indent.len() + "// ".len())
        .max(20);
    for line in text.lines() {
        let mut current = String::new();
        for word in line.split_whitespace() {
            if !current.is_empty() && current.len() + 1 + word.len() > width {
                lines.push(format!("{indent}// {current}"));
                current.clear();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        lines.push(format!("{indent}// {current}"));
    }
}
