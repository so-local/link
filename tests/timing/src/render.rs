use crate::{Group, MeasurementKind, Report};
use std::fmt::Write;

fn escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn seconds(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.2} s"))
        .unwrap_or_else(|| "unknown".into())
}

fn group_seconds(group: Option<&Group>) -> String {
    match group {
        None => "not observed".into(),
        Some(group) if group.unmeasured.max(group.incomplete) == group.programs => "unknown".into(),
        Some(group) if group.unmeasured.max(group.incomplete) > 0 => {
            format!("{:.2} s (partial)", group.seconds)
        }
        Some(group) => seconds(Some(group.seconds)),
    }
}

fn group_count(group: &Group, count: u64) -> String {
    if group.incomplete == group.programs {
        "unknown".into()
    } else if group.incomplete > 0 {
        format!("{count} (partial)")
    } else {
        count.to_string()
    }
}

fn phase_table(page: &mut String, report: &Report) {
    let Some(validation) = &report.validation else {
        page.push_str(match report.kind {
            MeasurementKind::TestPrograms => "<p class=\"note\">Validation phases were not captured. Test execution alone does not measure formatting, Clippy, builds, setup or web checks.</p>",
            MeasurementKind::Cases => "<p class=\"note\">Run phases were not captured. Case durations do not establish overall elapsed, setup or cleanup time.</p>",
        });
        return;
    };
    let mut summed = seconds(validation.summed_seconds());
    if validation.phases.iter().any(|p| p.wall_seconds.is_none()) {
        summed.push_str(" (partial coverage)");
    }
    let introduction = match report.kind {
        MeasurementKind::TestPrograms => {
            "<h2>Validation phases</h2><p>These are explicitly measured command wall times. Summed work can exceed overall elapsed when phases overlap. Do not add the test-program totals below to the test-command phase, which already includes execution and preparation.</p>"
        }
        MeasurementKind::Cases => {
            "<h2>Run phases</h2><p>These are explicitly measured controller phases. The execution-and-fixtures phase already includes case work. Case totals can overlap and must not be added to this phase or substituted for controller elapsed.</p>"
        }
    };
    let elapsed_label = match report.kind {
        MeasurementKind::TestPrograms => "Overall validation elapsed",
        MeasurementKind::Cases => "Overall controller elapsed",
    };
    write!(page, "{introduction}<dl><dt>{elapsed_label}</dt><dd>{}</dd><dt>Summed measured phase work</dt><dd>{}</dd><dt>Recorded cache context</dt><dd>{}</dd></dl><table><thead><tr><th>Phase</th><th class=\"num\">Wall time</th><th>Outcome</th></tr></thead><tbody>", seconds(validation.elapsed_seconds), summed, escape(validation.cache_state.as_deref().unwrap_or("unknown (not recorded)"))).unwrap();
    for phase in &validation.phases {
        let outcome = match phase.status {
            Some(0) => "passed (0)".into(),
            Some(status) => format!("failed ({status})"),
            None => "unknown / not recorded".into(),
        };
        write!(
            page,
            "<tr><td>{}</td><td class=\"num\">{}</td><td>{outcome}</td></tr>",
            escape(&phase.name),
            seconds(phase.wall_seconds)
        )
        .unwrap();
    }
    page.push_str("</tbody></table><p class=\"note\">Only recorded phases are measured. Omitted costs and unknown durations are not zero. Overall elapsed has its own capture boundaries and is never inferred from the sum. Cache warmth is not inferred from duration or a cache action.</p>");
    if validation
        .phases
        .iter()
        .any(|p| p.status.is_some_and(|status| status != 0))
    {
        page.push_str("<p class=\"warning\">At least one validation phase failed. Successful test execution does not make the whole validation cycle pass.</p>");
    }
    page.push_str(match report.kind {
        MeasurementKind::TestPrograms => "<h2>Test command and execution</h2>",
        MeasurementKind::Cases => "<h2>Case executions</h2>",
    });
}

fn ranked_table(page: &mut String, report: &Report) {
    page.push_str("<h2>Ranked areas</h2>");
    page.push_str(match report.kind {
        MeasurementKind::TestPrograms => "<p>Nested packages belong to the nearest containing module with a binary. Shared packages and repository tests retain their own areas. Total test time and share cover measured program durations only. Counts come from observed libtest summaries; missing summaries leave their counts unknown. Capability-gated early returns may appear as passed, so gate coverage is unknown from these counts.</p>",
        MeasurementKind::Cases => "<p>Cases are grouped by their recorded owning member. Each row retains its run, machine, control mode and retry count. Durations include all recorded attempts. Summed work can exceed elapsed time when cases overlap. Execution outcomes do not establish current proof or output approval.</p>",
    });
    if let Some(reason) = &report.context.filtered_skip_reason {
        write!(page, "<p>Skipped includes explicitly ignored libtest tests and all filtered tests, explicitly classified as environment skips. Classification reason: {}. Passed and failed counts are unchanged.</p>", escape(reason)).unwrap();
    } else if report.kind == MeasurementKind::Cases {
        page.push_str("<p>Skipped means explicitly skipped in a case record. Unrecorded cases and output awaiting review remain visible separately. Missing execution time is unknown, including a case skipped before execution.</p>");
    } else {
        page.push_str("<p>Skipped means explicitly ignored by libtest. Filtered tests are not classified as skipped.</p>");
    }
    page.push_str("<table><thead><tr><th>Area</th><th class=\"num\">Measured time</th><th class=\"num\">Share</th><th class=\"num\">Passed</th><th class=\"num\">Skipped</th><th class=\"num\">Failed</th></tr></thead><tbody>");
    for group in &report.groups {
        let share = if group.unmeasured.max(group.incomplete) == group.programs {
            "unknown".into()
        } else if report.measured_program_seconds > 0.0 {
            format!(
                "{:.1}%",
                group.seconds / report.measured_program_seconds * 100.0
            )
        } else {
            "not measurable".into()
        };
        let coverage = if group.incomplete > 0 {
            format!(
                "{} executions; {} without outcomes",
                group.programs, group.incomplete
            )
        } else {
            format!("{} executions", group.programs)
        };
        write!(page, "<tr><td>{}<br><small>{coverage}</small></td><td class=\"num\">{}</td><td class=\"num\">{share}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>", escape(&group.name), group_seconds(Some(group)), group_count(group, group.passed), group_count(group, group.skipped), group_count(group, group.failed)).unwrap();
    }
    page.push_str("</tbody></table>");
}

/// Self-contained HTML, without scripts or requests to external services.
pub fn html(report: &Report, baseline: Option<&Report>) -> String {
    const COLORS: [&str; 10] = [
        "#2563eb", "#0d9488", "#ea580c", "#9333ea", "#db2777", "#4d7c0f", "#0891b2", "#b45309",
        "#64748b", "#dc2626",
    ];
    let mut page = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Execution timings</title><style>body{font:16px system-ui,sans-serif;max-width:1100px;margin:40px auto;padding:0 20px;color:#172033;background:#fafbfd}h1,h2{line-height:1.2}.chart{display:flex;gap:32px;align-items:center;flex-wrap:wrap}.pie{width:230px;height:230px;border-radius:50%;flex-shrink:0}table{border-collapse:collapse;width:100%;margin:20px 0;font-variant-numeric:tabular-nums}th,td{text-align:left;border-bottom:1px solid #dce1e8;padding:9px;overflow-wrap:anywhere}td.num,th.num{text-align:right}small,.note{color:#526079}.warning{border-left:4px solid #d97706;padding:10px 16px;background:#fff7e6}.swatch{display:inline-block;width:12px;height:12px;margin-right:8px}code{overflow-wrap:anywhere}dt{font-weight:600}dd{margin:0 0 10px}a{color:#2563eb}@media(max-width:700px){body{margin:20px auto}table{font-size:13px}th,td{padding:6px}}</style><h1>Execution timings</h1>",
    );
    phase_table(&mut page, report);
    let completion = if report.kind == MeasurementKind::Cases {
        if report.complete {
            "The recorded run succeeded and every observed case has a measured passing execution. This is timing evidence, not a verdict on what the product does."
        } else {
            "Case timing evidence is partial, failed or unverified. Unknown costs and missing executions are not zero."
        }
    } else if report.complete {
        "Cargo succeeded and every observed program has a successful summary."
    } else {
        match report.context.status {
            Some(0) => {
                "Cargo succeeded, but timing coverage is incomplete. Do not treat this as a complete timing baseline."
            }
            Some(_) => {
                "Cargo failed. Available timings are partial and do not form a complete passing baseline."
            }
            None => {
                "Cargo exit status is unknown. Do not treat this as a verified passing baseline."
            }
        }
    };
    write!(page, "<p class=\"warning\">{completion}</p><p><strong>{:.2} seconds</strong> measured across {} executions. This is the sum of execution wall times, not CPU time or the total command duration. Overlapping execution durations can exceed elapsed time.</p><dl><dt>Total command wall time</dt><dd>{}</dd><dt>Build preparation time</dt><dd>{}</dd>", report.measured_program_seconds, report.programs.len(), seconds(report.context.wall_seconds), seconds(report.compilation_seconds)).unwrap();
    for (label, value) in [
        ("Revision", report.context.revision.as_deref()),
        ("Machine", report.context.machine.as_deref()),
        ("Concurrency", report.context.threads.as_deref()),
    ] {
        write!(
            page,
            "<dt>{label}</dt><dd>{}</dd>",
            escape(value.unwrap_or("not recorded"))
        )
        .unwrap();
    }
    write!(
        page,
        "<dt>Recorded command exit status</dt><dd>{}</dd></dl>",
        report
            .context
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".into())
    )
    .unwrap();
    for warning in &report.warnings {
        write!(page, "<p class=\"warning\">{}</p>", escape(warning)).unwrap();
    }
    page.push_str(match report.kind {
        MeasurementKind::TestPrograms => "<p class=\"note\">Timing precision is Cargo's printed precision. Ignored counts are explicit libtest ignores; capability-gated early returns may appear as passes. Record required capabilities when capturing a comparison.</p>",
        MeasurementKind::Cases => "<p class=\"note\">Timing precision is the saved case record's precision. Retry durations are already included once. Ordinary and control executions retain separate rows. Compare matching builds, environments, case selection, fixture settings and concurrency.</p>",
    });
    ranked_table(&mut page, report);
    page.push_str("<details><summary>Pie chart of measured execution time</summary><h2>Where measured execution time goes</h2>");
    let mut slices: Vec<(String, f64)> = report
        .groups
        .iter()
        .filter(|g| g.seconds > 0.0)
        .take(9)
        .map(|g| (g.name.clone(), g.seconds))
        .collect();
    let rest: f64 = report
        .groups
        .iter()
        .filter(|g| g.seconds > 0.0)
        .skip(9)
        .map(|g| g.seconds)
        .sum();
    if rest > 0.0 {
        slices.push(("Other groups".into(), rest));
    }
    let mut gradient = String::new();
    let mut angle = 0.0;
    for (index, (_, duration)) in slices.iter().enumerate() {
        let end = angle + duration / report.measured_program_seconds * 360.0;
        if index > 0 {
            gradient.push(',');
        }
        write!(gradient, "{} {angle:.6}deg {end:.6}deg", COLORS[index]).unwrap();
        angle = end;
    }
    if slices.is_empty() {
        page.push_str("<p>No positive measured durations are available. Missing durations remain unknown.</p>");
    } else {
        write!(page, "<div class=\"chart\"><div class=\"pie\" role=\"img\" aria-label=\"Share of measured execution time; values are in the adjacent legend\" style=\"background:conic-gradient({gradient})\"></div><div>").unwrap();
        for (index, (name, duration)) in slices.iter().enumerate() {
            write!(
                page,
                "<p><span class=\"swatch\" style=\"background:{}\"></span>{}: {:.2} s ({:.1}%)</p>",
                COLORS[index],
                escape(name),
                duration,
                duration / report.measured_program_seconds * 100.0
            )
            .unwrap();
        }
        page.push_str("</div></div>");
    }
    page.push_str("</details>");
    if baseline.is_some_and(|baseline| baseline.kind != report.kind) {
        page.push_str("<p class=\"warning\">The baseline measures a different execution model; no duration comparison is available.</p>");
    }
    if let Some(baseline) = baseline.filter(|baseline| baseline.kind == report.kind) {
        page.push_str("<h2>Previous measurement</h2><p>Differences are descriptive. Compare matching machines, thread counts, test selection and capability settings. Added or removed programs can change group totals.</p>");
        if !baseline.complete || !report.complete {
            page.push_str("<p class=\"warning\">At least one measurement is failed, incomplete or unverified.</p>");
        }
        page.push_str("<table><thead><tr><th>Group</th><th class=\"num\">Previous seconds</th><th class=\"num\">Current seconds</th><th class=\"num\">Change</th></tr></thead><tbody>");
        let names: std::collections::BTreeSet<_> = report
            .groups
            .iter()
            .chain(&baseline.groups)
            .map(|g| &g.name)
            .collect();
        for name in names {
            let previous = baseline.groups.iter().find(|g| g.name == *name);
            let current = report.groups.iter().find(|g| g.name == *name);
            let change = match previous.zip(current) {
                Some((a, b))
                    if a.incomplete == 0
                        && b.incomplete == 0
                        && a.unmeasured == 0
                        && b.unmeasured == 0 =>
                {
                    format!("{:+.2} s", b.seconds - a.seconds)
                }
                Some(_) => "incomplete group".into(),
                None => "selection changed".into(),
            };
            write!(page, "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{change}</td></tr>", escape(name), group_seconds(previous), group_seconds(current)).unwrap();
        }
        page.push_str("</tbody></table>");
    }
    page.push_str("<h2>Recorded executions</h2><table><thead><tr><th>Member / execution</th><th>Result</th><th class=\"num\">Seconds</th></tr></thead><tbody>");
    let mut programs: Vec<_> = report.programs.iter().collect();
    programs.sort_by(|a, b| {
        b.seconds
            .unwrap_or(-1.0)
            .total_cmp(&a.seconds.unwrap_or(-1.0))
    });
    for program in programs {
        write!(
            page,
            "<tr><td>{}<br><small>{}</small></td><td>{}</td><td class=\"num\">{}</td></tr>",
            escape(program.package.as_deref().unwrap_or("unassigned")),
            escape(&program.target),
            escape(program.outcome.as_deref().unwrap_or("no summary")),
            seconds(program.seconds)
        )
        .unwrap();
    }
    page.push_str("</tbody></table></html>");
    page
}
