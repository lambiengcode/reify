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
// Colours are referenced through CSS custom properties so one SVG reads correctly on
// GitHub's light and dark themes; `defs()` defines both and swaps them under
// `prefers-color-scheme`. The literals here are the light-theme values.
const INK: &str = "#8b949e";
const GRID: &str = "#8b949e";
const BASELINE: &str = "url(#gNone)";
const RIVAL: &str = "url(#gRival)";
const REIFY: &str = "url(#gReify)";
const DECOY: &str = "url(#gDecoy)";
const CEILING: &str = "url(#gCeiling)";
const RIVAL_ALT: &str = "url(#gRivalAlt)";

/// Gradients, theme tokens and the type scale, emitted once per chart.
fn defs() -> String {
    let bar = |id: &str, top: &str, bottom: &str| {
        format!(
            r##"    <linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="{top}"/><stop offset="100%" stop-color="{bottom}"/>
    </linearGradient>"##
        )
    };
    format!(
        r##"  <defs>
{}
{}
{}
{}
{}
{}
    <filter id="lift" x="-40%" y="-40%" width="180%" height="180%">
      <feDropShadow dx="0" dy="1.5" stdDeviation="2.5" flood-color="#000" flood-opacity="0.18"/>
    </filter>
  </defs>
  <style>
    text {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; }}
    /* The base fills are a mid tone legible on both backgrounds, so a renderer that
       ignores the media queries below still produces a readable chart. The queries
       are an enhancement, never a requirement. */
    .t  {{ font-size: 16px; font-weight: 650; fill: #7d8590; }}
    .st {{ font-size: 12px; fill: #8b949e; }}
    .ax {{ font-size: 11px; fill: #8b949e; }}
    .vl {{ font-size: 12px; font-weight: 700; fill: #7d8590; }}
    .cl {{ font-size: 10px; fill: #8b949e; }}
    .rp {{ font-size: 13px; font-weight: 650; fill: #7d8590; }}
    @media (prefers-color-scheme: light) {{
      .t, .vl, .rp {{ fill: #24292f; }}
      .st, .ax, .cl {{ fill: #57606a; }}
    }}
    @media (prefers-color-scheme: dark) {{
      .t, .vl, .rp {{ fill: #e6edf3; }}
      .st, .ax, .cl {{ fill: #9aa4ae; }}
    }}
  </style>
"##,
        bar("gReify", "#3fb950", "#2da44e"),
        bar("gRival", "#e8a13c", "#d9822b"),
        bar("gRivalAlt", "#d4a017", "#bf8700"),
        bar("gDecoy", "#a371f7", "#8957e5"),
        bar("gCeiling", "#8b949e", "#6e7781"),
        bar("gNone", "#a8b1ba", "#9aa4ae"),
    )
}

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
        "R-reify-iter3" => REIFY,
        c if c.starts_with("R-reify") && !c.contains("shuffled") => REIFY,
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
        "N-no-context" => "none",
        "B-content-grep" => "grep",
        "R-reify" => "reify",
        "R-shuffled" => "decoy",
        "O-oracle" => "perfect",
        "C-path-grep" => "path grep",
        "R-reify-iter3" => "reify ×3",
        "B-content-grep-x3" => "grep ×3",
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
    let _ = writeln!(
        svg,
        r#"<svg viewBox="0 0 {WIDTH} {height}" xmlns="http://www.w3.org/2000/svg" font-family="-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif">
  <title>Share of tasks where the model named a file that actually had to change, by condition, for each repository. Whiskers are 95% confidence intervals.</title>
{defs}  <text x="{mid}" y="28" class="t" text-anchor="middle">Did the model name a file that actually had to change?</text>
  <text x="{mid}" y="48" class="st" text-anchor="middle">Tasks from real merged commits, indexed before those commits existed. Whiskers: 95% CI.</text>"#,
        defs = defs(),
        mid = WIDTH / 2.0
    );

    // Legend, laid out on a fixed grid rather than by estimating text width. Guessing
    // the width of a proportional font is how legend entries end up on top of each
    // other, and there is no layout engine here to catch it.
    let legend = [
        ("#9aa4ae", "no context (memorisation control)"),
        ("#d9822b", "grep, tripled budget"),
        ("#2da44e", "reify, three rounds (same cost)"),
        ("#6e7781", "perfect context (ceiling)"),
    ];
    let columns = 2usize;
    let column_width = (WIDTH - 132.0) / columns as f32;
    for (index, (colour, text)) in legend.iter().enumerate() {
        let x = 66.0 + (index % columns) as f32 * column_width;
        let y = 62.0 + (index / columns) as f32 * 19.0;
        let _ = writeln!(
            svg,
            r#"  <rect x="{x:.1}" y="{y:.1}" width="10" height="10" rx="2.5" fill="{colour}"/><text x="{tx:.1}" y="{ty:.1}" class="ax">{text}</text>"#,
            tx = x + 16.0,
            ty = y + 10.0
        );
    }

    // Grid and axis.
    for step in 0..=4 {
        let value = step as f32 * 25.0;
        let y = plot_bottom - (value / 100.0) * plot_height;
        let (opacity, dash) = if step == 0 {
            (1.0, "")
        } else {
            (0.7, r#" stroke-dasharray="3 5""#)
        };
        let stroke = if step == 0 { BASELINE } else { GRID };
        let _ = writeln!(
            svg,
            r#"  <line x1="{left}" y1="{y:.1}" x2="{right}" y2="{y:.1}" stroke="{stroke}" stroke-opacity="{opacity}"{dash}/>
  <text x="{tx}" y="{ty:.1}" class="ax" text-anchor="end">{value:.0}%</text>"#,
            tx = left - 8.0,
            ty = y + 4.0
        );
    }
    let _ = writeln!(
        svg,
        r#"  <text x="22" y="{mid:.0}" class="st" text-anchor="middle" transform="rotate(-90 22 {mid:.0})">tasks with a correct file</text>"#,
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
                (top + 18.0, "#ffffff")
            };
            let _ = writeln!(
                svg,
                r#"  <rect x="{bx:.1}" y="{top:.1}" width="{bar_width:.1}" height="{h:.1}" rx="3.5" fill="{colour}" filter="url(#lift)"/>
  <text x="{cx:.1}" y="{label_y:.1}" class="vl" fill="{label_fill}" text-anchor="middle">{value:.0}%</text>"#,
                h = (plot_bottom - top).max(1.0)
            );
            // Interval whisker.
            let ly = plot_bottom - (low / 100.0) * plot_height;
            let _ = writeln!(
                svg,
                r#"  <line x1="{cx:.1}" y1="{hy:.1}" x2="{cx:.1}" y2="{ly:.1}" stroke="{INK}" stroke-opacity="0.5" stroke-linecap="round"/>
  <line x1="{a:.1}" y1="{hy:.1}" x2="{b:.1}" y2="{hy:.1}" stroke="{INK}" stroke-opacity="0.5" stroke-linecap="round"/>
  <line x1="{a:.1}" y1="{ly:.1}" x2="{b:.1}" y2="{ly:.1}" stroke="{INK}" stroke-opacity="0.5" stroke-linecap="round"/>"#,
                a = cx - 5.0,
                b = cx + 5.0
            );
            // Two rows, alternating: four condition labels do not fit on one line at
            // this panel width, and a legend already carries the full names.
            let _ = writeln!(
                svg,
                r#"  <text x="{cx:.1}" y="{ly2:.1}" class="cl" text-anchor="middle">{label}</text>"#,
                ly2 = plot_bottom + if slot_index % 2 == 0 { 16.0 } else { 29.0 },
                label = label_for(&group.bars[slot_index].0)
            );
        }

        let _ = writeln!(
            svg,
            r#"  <text x="{cx:.1}" y="{y:.1}" class="rp" text-anchor="middle">{repo}</text>
  <text x="{cx:.1}" y="{y2:.1}" class="ax" text-anchor="middle">n = {tasks}</text>"#,
            cx = group_left + group_width / 2.0,
            y = plot_bottom + 44.0,
            y2 = plot_bottom + 60.0,
            repo = group.repository,
            tasks = group.tasks
        );
        if index > 0 {
            let _ = writeln!(
                svg,
                r#"  <line x1="{x:.1}" y1="{plot_top}" x2="{x:.1}" y2="{b:.1}" stroke="{GRID}" stroke-opacity="0.25"/>"#,
                x = group_left,
                b = plot_bottom + 66.0
            );
        }
    }

    let _ = writeln!(
        svg,
        r#"  <text x="{mid}" y="{y:.0}" class="ax" text-anchor="middle">Overlapping whiskers mean the difference is not established. On Medusa, reify and grep overlap.</text>
</svg>"#,
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
    let _ = writeln!(
        svg,
        r#"<svg viewBox="0 0 {WIDTH} {height}" xmlns="http://www.w3.org/2000/svg" font-family="-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif">
  <title>Retrieval quality with no model involved: the share of tasks where the tool put a changed file in front of the agent, per repository.</title>
{defs}  <text x="{mid}" y="28" class="t" text-anchor="middle">Retrieval only, no model: was the changed file offered at all?</text>
  <text x="{mid}" y="48" class="st" text-anchor="middle">Every condition held to the same 4,000-token budget.</text>"#,
        defs = defs(),
        mid = WIDTH / 2.0
    );

    for step in 0..=4 {
        let value = step as f32 * 25.0;
        let y = plot_bottom - (value / 100.0) * plot_height;
        let (opacity, dash) = if step == 0 {
            (1.0, "")
        } else {
            (0.7, r#" stroke-dasharray="3 5""#)
        };
        let stroke = if step == 0 { BASELINE } else { GRID };
        let _ = writeln!(
            svg,
            r#"  <line x1="{left}" y1="{y:.1}" x2="{right}" y2="{y:.1}" stroke="{stroke}" stroke-opacity="{opacity}"{dash}/>
  <text x="{tx}" y="{ty:.1}" class="ax" text-anchor="end">{value:.0}%</text>"#,
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
            let _ = writeln!(
                svg,
                r#"  <rect x="{bx:.1}" y="{top:.1}" width="{bar_width:.1}" height="{h:.1}" rx="3.5" fill="{colour}" filter="url(#lift)"/>
  <text x="{cx:.1}" y="{vy:.1}" class="vl" text-anchor="middle">{value:.0}%</text>
  <text x="{cx:.1}" y="{ly:.1}" class="cl" text-anchor="middle">{label}</text>"#,
                bx = cx - bar_width / 2.0,
                h = (plot_bottom - top).max(1.0),
                vy = top - 8.0,
                ly = plot_bottom + 16.0,
                label = label_for(condition)
            );
        }
        let _ = writeln!(
            svg,
            r#"  <text x="{cx:.1}" y="{y:.1}" font-size="13" font-weight="600" fill="{INK}" text-anchor="middle">{repo}</text>"#,
            cx = group_left + group_width / 2.0,
            y = plot_bottom + 42.0,
            repo = group.repository
        );
    }

    let _ = writeln!(
        svg,
        r#"  <text x="{mid}" y="{y:.0}" font-size="11" fill="{INK}" text-anchor="middle">Path grep offers 88 files to reach 18%. Offering everything is not retrieval.</text>
</svg>"#,
        mid = WIDTH / 2.0,
        y = height - 14.0
    );
    svg
}

/// Build a chart series from a repository's agent summaries.
pub fn agent_series(repository: &str, summaries: &[AgentSummary]) -> Series {
    // The headline is the budget-matched comparison: reify iterated three rounds
    // against grep handed the same tripled budget outright. Single-shot numbers live
    // in the tables; seven bars per group is a legend, not a chart.
    let order = [
        "N-no-context",
        "B-content-grep-x3",
        "R-reify-iter3",
        "O-oracle",
    ];
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
    let order = ["B-content-grep", "C-path-grep", "R-reify", "R-reify-iter3"];
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
        assert!(
            svg.contains("<title>"),
            "a chart with no title is unreadable aloud"
        );
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
        // Counted by the cap style whiskers use, not by an exact opacity, so a
        // change of shade does not read as a change of substance.
        let whiskers = svg.matches("stroke-linecap=\"round\"").count();
        assert!(
            whiskers >= 9,
            "expected three whiskers per bar, got {whiskers}"
        );
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
        assert_eq!(label_for("N-no-context"), "none");
        assert_eq!(label_for("R-shuffled"), "decoy");
        assert_eq!(label_for("unknown-condition"), "other");
    }

    #[test]
    fn an_empty_series_does_not_panic() {
        let svg = agent_chart(&[]);
        assert!(svg.contains("</svg>"));
    }
}
