//! `chap-devops top`: the live tree on a terminal, one snapshot anywhere else.

use crate::cli::TopArgs;
use crate::commands::Ctx;
use crate::error::Result;
use crate::output::{Out, Report};
use crate::project::Project;
use crate::top::data::{self, Node};
use std::io::IsTerminal;
use std::path::PathBuf;

/// The deployments to show even with no container: the one the command runs
/// in, and every `chap-devops run` group.
fn known(ctx: &Ctx) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = super::run::groups().into_iter().map(|(_, d)| d).collect();
    if let Some(here) = Project::find_root(&ctx.project_dir) {
        dirs.insert(0, here);
    }
    dirs
}

/// The screen on a terminal; under `--json`, or with stdout not a terminal,
/// one snapshot of the same tree.
pub fn run(ctx: &Ctx, args: &TopArgs) -> Result<()> {
    if let Some(path) = &args.record {
        return record(ctx, args, path);
    }
    if ctx.out.json || !std::io::stdout().is_terminal() {
        let nodes = data::collect(&known(ctx));
        let value = serde_json::json!({ "deployments": nodes });
        if !ctx.out.json {
            print!("{}", snapshot(&nodes, &ctx.out));
        }
        return ctx
            .out
            .report(&value, |lines| snapshot_lines(&nodes, lines));
    }
    crate::top::run(ctx, known(ctx), args.interval)
}

/// `--record`: samples to a file until `--duration` or Ctrl-C, then one line
/// that says how much was written.
fn record(ctx: &Ctx, args: &TopArgs, path: &std::path::Path) -> Result<()> {
    crate::interrupt::install();
    let summary = crate::top::record::record(path, &known(ctx), args.interval, args.duration)
        .map_err(|e| {
            anyhow::anyhow!(
                "could not write the recording to `{}`: {e}; name a file in a directory you can write to",
                path.display()
            )
        })?;
    let value = serde_json::json!({ "file": path, "recording": summary });
    ctx.out.report(&value, |lines| {
        lines.info(format!(
            "recorded {} samples in {} ticks over {} s to `{}`",
            summary.lines,
            summary.ticks,
            summary.seconds,
            path.display()
        ));
        if summary.lines == 0 {
            lines.warning(
                "no chap-devops container was found, so the file has no samples; start a deployment with `chap-devops up` first",
            );
        }
    })
}

/// The lines under the tree: what was found, and that it is a snapshot.
fn snapshot_lines(nodes: &[Node], lines: &mut Report) {
    match nodes.is_empty() {
        true => lines
            .info("no chap-devops deployment has a container on this machine")
            .hint("`chap-devops run <model>` or `chap-devops up` starts one"),
        false => lines.hint("a snapshot: on a terminal `chap-devops top` keeps it up to date"),
    };
}

/// The tree as text, for a pipe; empty when there is no deployment to show.
fn snapshot(nodes: &[Node], out: &Out) -> String {
    let mut text = String::new();
    for node in nodes {
        text.push_str(&format!(
            "{}  {}  {}/{} running{}\n",
            node.name,
            node.kind.label(),
            node.running(),
            node.services.len(),
            node.dir
                .as_ref()
                .map(|d| format!("  {}", d.display()))
                .unwrap_or_default()
        ));
        let rows: Vec<Vec<String>> = node
            .services
            .iter()
            .map(|s| {
                vec![
                    format!("  {}", s.service),
                    match &s.health {
                        Some(h) => format!("{} ({h})", s.state),
                        None => s.state.clone(),
                    },
                    s.cpu.clone().unwrap_or_default(),
                    s.memory.clone().unwrap_or_default(),
                    s.url.clone().unwrap_or_default(),
                ]
            })
            .collect();
        text.push_str(&out.table(&["  SERVICE", "STATE", "CPU", "MEMORY", "URL"], &rows));
    }
    text
}
