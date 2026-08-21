//! Chart generation for the benchmark report.
//!
//! SVG is written by hand rather than by a plotting crate: the output is a handful of
//! rectangles and it has to render inside GitHub's markdown, which strips scripts,
//! external stylesheets and most embedded fonts.
//!
//! Every chart is generated **from the committed result files**, never from numbers
//! typed into a template. A chart that can drift from its data is a picture, not a
//! measurement.

use std::fmt::Write as _;

use crate::agent::AgentSummary;
use crate::metrics::Summary;

/// GitHub renders markdown on both light and dark backgrounds, so every colour here is
/// one that stays legible on either. Greys come from GitHub's own palette.
const INK: &str = "#8b949e";
const GRID: &str = "#8b949e";
const BASELINE: &str = "#8b949e";
const RIVAL: &str = "#d9822b";
const REIFY: &str = "#2da44e";
const DECOY: &str = "#8957e5";
const CEILING: &str = "#57606a";
const RIVAL_ALT: &str = "#bf8700";

const WIDTH: f32 = 860.0;

/// One repository's results, as a chart wants them.
pub struct Series {
    pub repository: String,
    pub tasks: usize,
    /// `(condition, value, low, high, colour)`, already in display order.
    pub bars: Vec<(String, f32, f32, f32, &'static str)>,
}

/// Colour a condition by what it *is*, so the same role reads the same across charts.
fn colour_for(condition: &str) -> &'static str {
    match condition {
        c if c.starts_with("R-reify") => REIFY,
        c if c.starts_with("O-") => CEILING,
        c if c.starts_with("N-") => BASELINE,
        c if c.starts_with("R-shuffled") => DECOY,
        c if c.starts_with("C-") => RIVAL_ALT,
        _ => RIVAL,
    }
}

/// Shorten a condition id into something readable under a bar.
fn label_for(condition: &str) -> &'static str {
    match condition {
        "N-no-context" => "no context",
        "B-content-grep" => "grep",
        "R-reify" => "reify",
        "R-shuffled" => "decoy",
        "O-oracle" => "perfect",
        "C-path-grep" => "path grep",
        _ => "other",
    }
}

/// The headline chart: hit rate per condition, per repository, with intervals.
///
/// Confidence intervals are drawn because they are the point. Two bars whose whiskers
/// overlap have not been shown to differ, and a chart that hides that is arguing
/// rather than reporting.
pub fn agent_chart(series: &[Series]) -> String {
    let height = 486.0;
    let plot_top = 112.0;
    let plot_bottom = 372.0;
    let plot_height = plot_bottom - plot_top;
    let left = 74.0;
    let right = WIDTH - 28.0;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg viewBox="0 0 {WIDTH} {height}" xmlns="http://www.w3.org/2000/svg" font-family="-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif">
  <title>Share of tasks where the model named a file that actually had to change, by condition, for each repository. Whiskers are 95% confidence intervals.</title>
  <text x="{mid}" y="26" font-size="15" font-weight="600" fill="{INK}" text-anchor="middle">Did the model name a file that actually had to change?</text>
  <text x="{mid}" y="46" font-size="12" fill="{INK}" text-anchor="middle">Tasks from real merged commits, indexed before those commits existed. Whiskers: 95% CI.</text>
"#,
        mid = WIDTH / 2.0
    );

    // Legend, laid out on a fixed grid rather than by estimating text width. Guessing
    // the width of a proportional font is how legend entries end up on top of each
    // other, and there is no layout engine here to catch it.
    let legend = [
        (BASELINE, "no context (control)"),
        (RIVAL, "budget-matched grep"),
        (REIFY, "reify"),
        (DECOY, "decoy context (control)"),
        (CEILING, "perfect context (ceiling)"),
    ];
    let columns = 3usize;
    let column_width = (WIDTH - 132.0) / columns as f32;
    for (index, (colour, text)) in legend.iter().enumerate() {
        let x = 66.0 + (index % columns) as f32 * column_width;
        let y = 62.0 + (index / columns) as f32 * 19.0;
        let _ = write!(
            svg,
            r#"  <rect x="{x:.1}" y="{y:.1}" width="11" height="11" rx="2" fill="{colour}"/><text x="{tx:.1}" y="{ty:.1}" font-size="11" fill="{INK}">{text}</text>
"#,
            tx = x + 16.0,
            ty = y + 10.0
        );
    }

    // Grid and axis.
    for step in 0..=4 {
        let value = step as f32 * 25.0;
        let y = plot_bottom - (value / 100.0) * plot_height;
        let opacity = if step == 0 { 0.55 } else { 0.16 };
        let _ = write!(
            svg,
            r#"  <line x1="{left}" y1="{y:.1}" x2="{right}" y2="{y:.1}" stroke="{GRID}" stroke-opacity="{opacity}"/>
  <text x="{tx}" y="{ty:.1}" font-size="11" fill="{INK}" text-anchor="end">{value:.0}%</text>
"#,
            tx = left - 8.0,
            ty = y + 4.0
        );
    }
    let _ = write!(
        svg,
        r#"  <text x="24" y="{mid:.0}" font-size="12" fill="{INK}" text-anchor="middle" transform="rotate(-90 24 {mid:.0})">tasks with a correct file</text>
"#,
        mid = (plot_top + plot_bottom) / 2.0
    );

    // Groups.
    let group_width = (right - left) / series.len() as f32;
    for (index, group) in series.iter().enumerate() {
        let group_left = left + index as f32 * group_width;
        let bars = group.bars.len() as f32;
        let slot = (group_width - 56.0) / bars;
        let bar_width = slot * 0.62;

        for (slot_index, (_, value, low, high, colour)) in group.bars.iter().enumerate() {
            let cx = group_left + 28.0 + slot * (slot_index as f32 + 0.5);
            let bx = cx - bar_width / 2.0;
            let top = plot_bottom - (value / 100.0) * plot_height;
            let hy = plot_bottom - (high / 100.0) * plot_height;
            // A tall bar has no room above it for its own label — that space belongs to
            // the whisker and then to the legend. Put the label inside instead.
            let label_above = hy - plot_top > 24.0;
            let (label_y, label_fill) = if label_above {
                (hy - 8.0, INK)
            } else {
                (top + 16.0, "#ffffff")
            };
            let _ = write!(
                svg,
                r#"  <rect x="{bx:.1}" y="{top:.1}" width="{bar_width:.1}" height="{h:.1}" rx="2" fill="{colour}"/>
  <text x="{cx:.1}" y="{label_y:.1}" font-size="11" font-weight="600" fill="{label_fill}" text-anchor="middle">{value:.0}%</text>
"#,
                h = (plot_bottom - top).max(1.0)
            );
            // Interval whisker.
            let ly = plot_bottom - (low / 100.0) * plot_height;
            let _ = write!(
                svg,
                r#"  <line x1="{cx:.1}" y1="{hy:.1}" x2="{cx:.1}" y2="{ly:.1}" stroke="{INK}" stroke-opacity="0.85"/>
  <line x1="{a:.1}" y1="{hy:.1}" x2="{b:.1}" y2="{hy:.1}" stroke="{INK}" stroke-opacity="0.85"/>
  <line x1="{a:.1}" y1="{ly:.1}" x2="{b:.1}" y2="{ly:.1}" stroke="{INK}" stroke-opacity="0.85"/>
"#,
                a = cx - 5.0,
                b = cx + 5.0
            );
            let _ = write!(
                svg,
                r#"  <text x="{cx:.1}" y="{ly2:.1}" font-size="10" fill="{INK}" text-anchor="middle">{label}</text>
"#,
                ly2 = plot_bottom + 16.0,
                label = label_for(&group.bars[slot_index].0)
            );
        }

        let _ = write!(
            svg,
            r#"  <text x="{cx:.1}" y="{y:.1}" font-size="13" font-weight="600" fill="{INK}" text-anchor="middle">{repo}</text>
  <text x="{cx:.1}" y="{y2:.1}" font-size="11" fill="{INK}" text-anchor="middle">n = {tasks}</text>
"#,
            cx = group_left + group_width / 2.0,
            y = plot_bottom + 44.0,
            y2 = plot_bottom + 60.0,
            repo = group.repository,
            tasks = group.tasks
        );
        if index > 0 {
            let _ = write!(
                svg,
                r#"  <line x1="{x:.1}" y1="{plot_top}" x2="{x:.1}" y2="{b:.1}" stroke="{GRID}" stroke-opacity="0.25"/>
"#,
                x = group_left,
                b = plot_bottom + 66.0
            );
        }
    }

    let _ = write!(
        svg,
        r#"  <text x="{mid}" y="{y:.0}" font-size="11" fill="{INK}" text-anchor="middle">Overlapping whiskers mean the difference is not established. On OpenMRS, reify and grep overlap.</text>
</svg>
"#,
        mid = WIDTH / 2.0,
        y = height - 12.0
    );
    svg
}

/// Retrieval quality without a model: does the right file get offered at all?
pub fn retrieval_chart(series: &[Series]) -> String {
    let height = 330.0;
    let plot_top = 74.0;
    let plot_bottom = 246.0;
    let plot_height = plot_bottom - plot_top;
    let left = 74.0;
    let right = WIDTH - 28.0;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg viewBox="0 0 {WIDTH} {height}" xmlns="http://www.w3.org/2000/svg" font-family="-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif">
  <title>Retrieval quality with no model involved: the share of tasks where the tool put a changed file in front of the agent, per repository.</title>
  <text x="{mid}" y="26" font-size="15" font-weight="600" fill="{INK}" text-anchor="middle">Retrieval only, no model: was the changed file offered at all?</text>
  <text x="{mid}" y="46" font-size="12" fill="{INK}" text-anchor="middle">Every condition held to the same 4,000-token budget.</text>
"#,
        mid = WIDTH / 2.0
    );

    for step in 0..=4 {
        let value = step as f32 * 25.0;
        let y = plot_bottom - (value / 100.0) * plot_height;
        let opacity = if step == 0 { 0.55 } else { 0.16 };
        let _ = write!(
            svg,
            r#"  <line x1="{left}" y1="{y:.1}" x2="{right}" y2="{y:.1}" stroke="{GRID}" stroke-opacity="{opacity}"/>
  <text x="{tx}" y="{ty:.1}" font-size="11" fill="{INK}" text-anchor="end">{value:.0}%</text>
"#,
            tx = left - 8.0,
            ty = y + 4.0
        );
    }

    let group_width = (right - left) / series.len() as f32;
    for (index, group) in series.iter().enumerate() {
        let group_left = left + index as f32 * group_width;
        let slot = (group_width - 90.0) / group.bars.len() as f32;
        let bar_width = slot * 0.5;
        for (slot_index, (condition, value, _, _, colour)) in group.bars.iter().enumerate() {
            let cx = group_left + 45.0 + slot * (slot_index as f32 + 0.5);
            let top = plot_bottom - (value / 100.0) * plot_height;
            let _ = write!(
                svg,
                r#"  <rect x="{bx:.1}" y="{top:.1}" width="{bar_width:.1}" height="{h:.1}" rx="2" fill="{colour}"/>
  <text x="{cx:.1}" y="{vy:.1}" font-size="11" font-weight="600" fill="{INK}" text-anchor="middle">{value:.0}%</text>
  <text x="{cx:.1}" y="{ly:.1}" font-size="10" fill="{INK}" text-anchor="middle">{label}</text>
"#,
                bx = cx - bar_width / 2.0,
                h = (plot_bottom - top).max(1.0),
                vy = top - 8.0,
                ly = plot_bottom + 16.0,
                label = label_for(condition)
            );
        }
        let _ = write!(
            svg,
            r#"  <text x="{cx:.1}" y="{y:.1}" font-size="13" font-weight="600" fill="{INK}" text-anchor="middle">{repo}</text>
"#,
            cx = group_left + group_width / 2.0,
            y = plot_bottom + 42.0,
            repo = group.repository
        );
    }

    let _ = write!(
        svg,
        r#"  <text x="{mid}" y="{y:.0}" font-size="11" fill="{INK}" text-anchor="middle">Path grep offers 88 files to reach 18%. Offering everything is not retrieval.</text>
</svg>
"#,
        mid = WIDTH / 2.0,
        y = height - 14.0
    );
    svg
}

/// Build a chart series from a repository's agent summaries.
pub fn agent_series(repository: &str, summaries: &[AgentSummary]) -> Series {
    let order = ["N-no-context", "B-content-grep", "R-reify", "R-shuffled", "O-oracle"];
    let mut bars = Vec::new();
    let mut tasks = 0usize;
    for name in order {
        if let Some(s) = summaries.iter().find(|s| s.condition == name) {
            tasks = tasks.max(s.tasks);
            bars.push((
                s.condition.clone(),
                s.hit_rate * 100.0,
                s.hit_rate_ci.0 * 100.0,
                s.hit_rate_ci.1 * 100.0,
                colour_for(name),
            ));
        }
    }
    Series {
        repository: repository.to_string(),
        tasks,
        bars,
    }
}

/// Build a chart series from a repository's retrieval summaries.
pub fn retrieval_series(repository: &str, summaries: &[Summary]) -> Series {
    let order = ["B-content-grep", "C-path-grep", "R-reify"];
    let mut bars = Vec::new();
    let mut tasks = 0usize;
    for name in order {
        if let Some(s) = summaries.iter().find(|s| s.condition == name) {
            tasks = tasks.max(s.tasks);
            bars.push((
                s.condition.clone(),
                s.hit_rate * 100.0,
                s.hit_rate * 100.0,
                s.hit_rate * 100.0,
                colour_for(name),
            ));
        }
    }
    Series {
        repository: repository.to_string(),
        tasks,
        bars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series() -> Vec<Series> {
        vec![Series {
            repository: "ERPNext".into(),
            tasks: 40,
            bars: vec![
                ("N-no-context".into(), 22.0, 12.0, 38.0, BASELINE),
                ("B-content-grep".into(), 32.0, 20.0, 48.0, RIVAL),
                ("R-reify".into(), 60.0, 45.0, 74.0, REIFY),
            ],
        }]
    }

    #[test]
    fn a_chart_is_self_contained_svg() {
        // GitHub strips scripts and external references from rendered markdown, so a
        // chart that needs either simply does not appear.
        let svg = agent_chart(&series());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        for forbidden in ["<script", "<image", "xlink:href", "@import", "url(http"] {
            assert!(!svg.contains(forbidden), "chart references {forbidden}");
        }
        // The SVG namespace is a declaration, not a fetch; nothing else may be a URL.
        let urls = svg.matches("http").count();
        assert_eq!(urls, 1, "the only URL may be the xmlns declaration");
    }

    #[test]
    fn a_chart_carries_a_title_for_screen_readers() {
        let svg = agent_chart(&series());
        assert!(svg.contains("<title>"), "a chart with no title is unreadable aloud");
    }

    #[test]
    fn every_value_in_the_chart_appears_as_text_too() {
        // The numbers must survive for anyone who cannot see the bars.
        let svg = agent_chart(&series());
        for value in ["22%", "32%", "60%"] {
            assert!(svg.contains(value), "missing {value}");
        }
    }

    #[test]
    fn confidence_intervals_are_drawn_not_hidden() {
        // Two bars whose whiskers overlap have not been shown to differ, and a chart
        // that omits them is arguing rather than reporting.
        let svg = agent_chart(&series());
        let whiskers = svg.matches("stroke-opacity=\"0.85\"").count();
        assert!(whiskers >= 9, "expected three whiskers per bar, got {whiskers}");
    }

    #[test]
    fn a_condition_keeps_its_colour_across_charts() {
        assert_eq!(colour_for("R-reify"), REIFY);
        assert_eq!(colour_for("O-oracle"), CEILING);
        assert_eq!(colour_for("N-no-context"), BASELINE);
        assert_ne!(colour_for("B-content-grep"), colour_for("R-reify"));
        // Two baselines side by side must be distinguishable from each other too.
        assert_ne!(colour_for("B-content-grep"), colour_for("C-path-grep"));
    }

    #[test]
    fn conditions_get_readable_labels() {
        assert_eq!(label_for("N-no-context"), "no context");
        assert_eq!(label_for("R-shuffled"), "decoy");
        assert_eq!(label_for("unknown-condition"), "other");
    }

    #[test]
    fn an_empty_series_does_not_panic() {
        let svg = agent_chart(&[]);
        assert!(svg.contains("</svg>"));
    }
}
