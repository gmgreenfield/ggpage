//! Detect and highlight MySQL `EXPLAIN FORMAT=TREE` / `EXPLAIN ANALYZE` output.
//!
//! Those plans are already an indented `->` tree; the value we add is flagging
//! the costly steps. We read each node's cost (`cost=`) or, for ANALYZE, its
//! actual time, and emphasize the nodes at/above half the maximum. Pure.

/// True if these lines look like a tree-format EXPLAIN plan.
pub fn looks_like_explain(lines: &[String]) -> bool {
    let mut arrows = 0;
    let mut metrics = 0;
    for l in lines {
        if l.contains("->") {
            arrows += 1;
        }
        if l.contains("cost=") || l.contains("actual time=") {
            metrics += 1;
        }
    }
    arrows >= 1 && metrics >= 1
}

/// The cost-like metric for a plan line: the estimated `cost=`, else the ANALYZE
/// per-loop `actual time=lo..hi` upper bound, else `None`.
fn node_metric(line: &str) -> Option<f64> {
    if let Some(rest) = line.split("cost=").nth(1) {
        return parse_leading_f64(rest);
    }
    if let Some(rest) = line.split("actual time=").nth(1) {
        // format: lo..hi — take the upper bound.
        let hi = rest.split("..").nth(1).unwrap_or(rest);
        return parse_leading_f64(hi);
    }
    None
}

fn parse_leading_f64(s: &str) -> Option<f64> {
    let t = s.trim_start();
    let end = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E'))
        .unwrap_or(t.len());
    t[..end].parse::<f64>().ok()
}

/// A rendered plan: the (possibly reflowed) lines plus a parallel flag marking
/// the costly nodes for the UI to emphasize.
pub struct ExplainPlan {
    pub lines: Vec<String>,
    pub emphasis: Vec<bool>,
}

/// Build an [`ExplainPlan`] from raw plan lines, flagging nodes whose metric is
/// at least half of the maximum observed.
pub fn render(lines: &[String]) -> ExplainPlan {
    let metrics: Vec<Option<f64>> = lines.iter().map(|l| node_metric(l)).collect();
    let max = metrics.iter().filter_map(|m| *m).fold(0.0_f64, f64::max);
    let threshold = max * 0.5;

    let emphasis = metrics
        .iter()
        .map(|m| matches!(m, Some(v) if max > 0.0 && *v > threshold))
        .collect();

    ExplainPlan {
        lines: lines.to_vec(),
        emphasis,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    const TREE: &str = "-> Nested loop inner join  (cost=1.40 rows=2)
    -> Table scan on t1  (cost=0.45 rows=2)
    -> Index lookup on t2 using idx  (cost=0.35 rows=1)";

    #[test]
    fn detects_tree_explain() {
        assert!(looks_like_explain(&lines(TREE)));
        assert!(!looks_like_explain(&lines("id\tname\n1\ta")));
        assert!(!looks_like_explain(&lines("just some prose")));
    }

    #[test]
    fn flags_the_costliest_nodes() {
        let plan = render(&lines(TREE));
        // max cost is 1.40; threshold 0.70 → only the root join qualifies.
        assert_eq!(plan.emphasis, vec![true, false, false]);
    }

    #[test]
    fn analyze_uses_actual_time_upper_bound() {
        let analyze = "-> Nested loop  (cost=1.0 rows=2) (actual time=0.1..9.0 rows=2 loops=1)
    -> Table scan on t  (cost=0.5 rows=2) (actual time=0.05..0.08 rows=2 loops=1)";
        // cost= appears first, so cost drives it here; the root (1.0) is costliest.
        let plan = render(&lines(analyze));
        assert_eq!(plan.emphasis, vec![true, false]);
    }

    #[test]
    fn pure_actual_time_without_cost() {
        let l = vec![
            "-> op A  (actual time=0.1..10.0 rows=1 loops=1)".to_string(),
            "-> op B  (actual time=0.1..2.0 rows=1 loops=1)".to_string(),
        ];
        let plan = render(&l);
        assert_eq!(plan.emphasis, vec![true, false]);
    }

    #[test]
    fn no_metrics_means_no_emphasis() {
        let l = vec!["-> op A".to_string(), "-> op B".to_string()];
        let plan = render(&l);
        assert_eq!(plan.emphasis, vec![false, false]);
    }
}
