//! voxcell-uphill CLI (spec §9 / P4-04): run the headless scene suite and
//! print the ported blueprint score report.
//!
//! Usage:
//!   voxcell-uphill                 # full suite, human summary + JSON
//!   voxcell-uphill --scene idle    # one scene
//!   voxcell-uphill --json          # JSON only (machine readable)
//!   voxcell-uphill --out PATH      # also write the JSON report to PATH
//!
//! Exit code: 0 when every scene audited clean, 1 on hard violations or
//! blocked scenes (gaps are printed either way).

use voxcell_uphill::{run_suite, scenes};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scene_filter: Option<String> = None;
    let mut json_only = false;
    let mut out_path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--scene" => {
                i += 1;
                scene_filter = args.get(i).cloned();
            }
            "--json" => json_only = true,
            "--out" => {
                i += 1;
                out_path = args.get(i).cloned();
            }
            "--help" | "-h" => {
                println!("voxcell-uphill [--scene NAME] [--json] [--out PATH]");
                println!("scenes: {}", scenes::SCENES.iter().map(|s| s.name).collect::<Vec<_>>().join(", "));
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let report = match scene_filter {
        Some(name) => {
            let Some(spec) = scenes::spec_by_name(&name) else {
                eprintln!("unknown scene: {name}");
                std::process::exit(2);
            };
            let scene = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scenes::run_scene(spec)
            }))
            .unwrap_or_else(|e| {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown panic".into());
                voxcell_uphill::SceneReport::blocked(spec.name, spec.size, msg)
            });
            let breakdown = voxcell_uphill::scoring::score_suite(std::slice::from_ref(&scene));
            let total = breakdown.total();
            voxcell_uphill::SuiteReport { scenes: vec![scene], breakdown, total_score: total, max_score: 100.0 }
        }
        None => run_suite(),
    };

    let json = serde_json::to_string_pretty(&report.to_json()).unwrap();
    if let Some(path) = &out_path {
        if let Err(e) = std::fs::write(path, &json) {
            eprintln!("failed to write {path}: {e}");
        }
    }
    if json_only {
        println!("{json}");
    } else {
        println!("== voxcell-uphill headless suite ==");
        for s in &report.scenes {
            let mut flags = Vec::new();
            if s.status != "ok" {
                flags.push(format!("BLOCKED: {}", s.gap.clone().unwrap_or_default()));
            } else {
                if !s.a11y.complete() { flags.push("a11y-incomplete".into()); }
                if !s.clipping.clean() { flags.push(format!("clipped-text({})", s.clipping.clipped.len())); }
                if !s.contrast.below_30.is_empty() { flags.push(format!("contrast<3({})", s.contrast.below_30.len())); }
                if let Some(p) = &s.popover {
                    if !p.edge_safe() { flags.push("popover-unsafe".into()); }
                }
                if let Some(p) = &s.perf {
                    flags.push(format!("frame p95 {:.2}ms{}", p.p95_ms, if p.within_budget { "" } else { " OVER-BUDGET" }));
                }
                if let Some(m) = &s.motion {
                    flags.push(format!("motion={}", m.moving));
                }
            }
            println!("{:<16} [{}] {}", s.scene, s.status, flags.join(" | "));
        }
        println!("score: {:.2}/100  breakdown: {}", report.total_score, report.breakdown.to_json());
        for sc in &report.scenes {
            for g in &sc.a11y.known_gaps {
                println!("known-gap [{}]: {g}", sc.scene);
            }
            for g in &sc.contrast.known_gaps {
                println!("known-contrast [{}]: {g}", sc.scene);
            }
        }
        let gaps = report.gaps();
        if !gaps.is_empty() {
            println!("gaps:");
            for (scene, reason) in &gaps {
                println!("  {scene}: {reason}");
            }
        }
    }

    let hard_fail = report.scenes.iter().any(|s| {
        s.status != "ok"
            || !s.a11y.complete()
            || !s.clipping.clean()
            || s.popover.as_ref().map(|p| !p.edge_safe()).unwrap_or(false)
    });
    std::process::exit(if hard_fail { 1 } else { 0 });
}
