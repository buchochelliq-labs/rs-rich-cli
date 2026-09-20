//! CLI-only batch planning and bounded process execution.
//!
//! Fail-fast stops scheduling as soon as any active worker fails; workers already
//! started finish, and their terminal output is replayed in resource order.
use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

type Failure = (String, ExitClass, Option<String>);

/// Resolve aliases through the nearest existing ancestor without creating it.
fn destination_key(path: &str) -> PathBuf {
    let resolved = resolved_destination_key(path);
    // Conservatively reject case aliases on Windows, including absent leaves.
    #[cfg(windows)]
    let resolved = PathBuf::from(resolved.to_string_lossy().to_lowercase());
    resolved
}

fn resolved_destination_key(path: &str) -> PathBuf {
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    // Ask the filesystem before simplifying `..`: link/../file is relative
    // to the symlink's target, not to the lexical parent of the link.
    let mut ancestor = absolute.clone();
    let mut suffix = Vec::new();
    let mut result = loop {
        if let Ok(canonical) = ancestor.canonicalize() {
            break canonical;
        }
        let Some(component) = ancestor.components().next_back() else {
            return absolute;
        };
        suffix.push(component.as_os_str().to_os_string());
        if !ancestor.pop() {
            return absolute;
        }
    };
    for component in suffix.into_iter().rev() {
        if component == ".." {
            result.pop();
        } else if component != "." {
            result.push(component);
        }
    }
    result
}

fn failure_class(failures: &[Failure]) -> ExitClass {
    failures
        .iter()
        .map(|(_, class, _)| *class)
        .max_by_key(|class| class.code())
        .unwrap_or(ExitClass::Success)
}

fn failure_json(failures: &[Failure]) -> Vec<serde_json::Value> {
    failures.iter().map(|(resource, class, message)| serde_json::json!({
        "resource": resource, "code": class.name(), "exit_code": class.code(), "message": message,
    })).collect()
}

pub(super) fn run_batch(cli: &Cli) -> ExitCode {
    let resources = match expand_batch_resources(&cli.resources) {
        Ok(resources) => resources,
        Err(message) => return fail(cli, ExitClass::Input, message),
    };
    let mut taken = std::collections::BTreeSet::new();
    let mut keys = std::collections::BTreeSet::new();
    let input_keys: std::collections::BTreeSet<_> = resources
        .iter()
        .map(|input| destination_key(input))
        .collect();
    let mut plans = Vec::with_capacity(resources.len());
    let mut errors = Vec::new();
    for (index, input) in resources.iter().enumerate() {
        let mut outputs = BatchOutputs::default();
        for (candidate, slot) in [
            (
                batch_export_path(cli.export_html.as_deref(), input, index, resources.len()),
                &mut outputs.html,
            ),
            (
                batch_export_path(cli.export_svg.as_deref(), input, index, resources.len()),
                &mut outputs.svg,
            ),
        ] {
            let Some(candidate) = candidate else { continue };
            *slot = Some(candidate.clone());
            match resolve_destination(cli, candidate, &mut taken) {
                Ok(path) => {
                    let key = destination_key(&path);
                    if input_keys.contains(&key) {
                        errors.push((
                            input.clone(),
                            ExitClass::Usage,
                            Some(format!(
                                "batch output would overwrite a batch input: {path}"
                            )),
                        ));
                    }
                    if !keys.insert(key)
                        && (cli.jobs > 1 || cli.collision == CollisionPolicy::Error)
                    {
                        errors.push((input.clone(), ExitClass::Usage, Some(format!("batch output collision: {path}; use --collision suffix (repeated overwrite destinations require --jobs 1)"))));
                    }
                    if cli.dry_run {
                        if Path::new(&path).is_dir() {
                            errors.push((
                                input.clone(),
                                ExitClass::Input,
                                Some(format!("output is a directory: {path}")),
                            ));
                        }
                        if let Some(parent) = Path::new(&path)
                            .parent()
                            .filter(|parent| !parent.as_os_str().is_empty())
                        {
                            if !parent.is_dir() {
                                errors.push((
                                    input.clone(),
                                    ExitClass::Input,
                                    Some(format!(
                                        "output parent is not an existing directory: {}",
                                        parent.display()
                                    )),
                                ));
                            }
                        }
                    }
                    *slot = Some(path);
                }
                Err((class, message)) => errors.push((input.clone(), class, Some(message))),
            }
        }
        plans.push(outputs);
    }
    if cli.dry_run {
        let class = failure_class(&errors);
        if cli.report_format == ReportFormat::Json {
            eprintln!(
                "{}",
                serde_json::json!({
                    "ok": errors.is_empty(), "code": class.name(), "exit_code": class.code(),
                    "result": { "dry_run": true, "planned": resources.len(), "attempted": 0,
                        "resources": resources.iter().zip(&plans).map(|(input, out)| serde_json::json!({
                            "resource": input, "html": out.html, "svg": out.svg,
                        })).collect::<Vec<_>>(), "errors": failure_json(&errors) }
                })
            );
        } else {
            for (input, out) in resources.iter().zip(&plans) {
                println!("{input}");
                if let Some(path) = &out.html {
                    println!("  HTML: {path}");
                }
                if let Some(path) = &out.svg {
                    println!("  SVG: {path}");
                }
            }
            for (input, _, message) in &errors {
                println!(
                    "  Error ({input}): {}",
                    message.as_deref().unwrap_or("failed")
                );
            }
            println!(
                "Dry run: {} file(s), {} error(s); no files written.",
                resources.len(),
                errors.len()
            );
        }
        return class.exit_code();
    }
    if let Some((_, class, message)) = errors.first() {
        return fail(
            cli,
            *class,
            message.as_deref().unwrap_or("invalid batch plan"),
        );
    }
    let (attempted, failures) = if cli.jobs > 1 {
        parallel(cli, &resources, &plans)
    } else {
        let mut failures = Vec::new();
        let mut attempted = 0;
        for (input, outputs) in resources.iter().zip(&plans) {
            let mut one = cli.clone();
            one.batch = false;
            one.resources = vec![input.clone()];
            one.resource = Some(input.clone());
            one.report_format = ReportFormat::Human;
            one.export_html = outputs.html.clone();
            one.export_svg = outputs.svg.clone();
            attempted += 1;
            if let Some((class, message)) = run_captured(one) {
                failures.push((input.clone(), class, message));
                if !cli.continue_on_error {
                    break;
                }
            }
        }
        (attempted, failures)
    };
    let class = failure_class(&failures);
    if cli.report_format == ReportFormat::Json {
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": failures.is_empty(), "code": class.name(), "exit_code": class.code(),
                "result": { "planned": resources.len(), "attempted": attempted,
                    "completed": attempted - failures.len(), "failed": failures.len(),
                    "skipped": resources.len() - attempted, "failures": failure_json(&failures) }
            })
        );
    } else {
        for (input, class, message) in &failures {
            eprintln!(
                "rich: {input}: {}",
                message.as_deref().unwrap_or(class.name())
            );
        }
    }
    class.exit_code()
}

struct Worker {
    index: usize,
    child: Child,
    stdout: std::fs::File,
    stderr: std::fs::File,
}

// Spooling changes the child's physical stdout, so forward the parent's
// rendering context separately. Paging continues to inspect the real TTY.
fn worker_terminal(command: &mut Command, terminal: bool, width: usize) {
    command.env("RS_RICH_BATCH_TERMINAL", if terminal { "1" } else { "0" });
    command.env("COLUMNS", width.to_string());
}

fn spawn(cli: &Cli, index: usize, input: &str, outputs: &BatchOutputs) -> std::io::Result<Worker> {
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let mut command = Command::new(std::env::current_exe()?);
    worker_terminal(
        &mut command,
        std::io::stdout().is_terminal(),
        Console::new().width(),
    );
    command.args(&cli.worker_args).args(["--report", "json"]);
    if let Some(path) = &outputs.html {
        command.args(["--export-html", path]);
    }
    if let Some(path) = &outputs.svg {
        command.args(["--export-svg", path]);
    }
    let child = command
        .args(["--", input])
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()?;
    Ok(Worker {
        index,
        child,
        stdout,
        stderr,
    })
}

fn child_failure(
    status: std::process::ExitStatus,
    stderr: &mut std::fs::File,
) -> Option<(ExitClass, Option<String>)> {
    if status.success() {
        return None;
    }
    let class = match status.code() {
        Some(2) => ExitClass::Usage,
        Some(4) => ExitClass::Data,
        Some(5) => ExitClass::Gate,
        _ => ExitClass::Input,
    };
    let mut bytes = Vec::new();
    let _ = stderr.seek(SeekFrom::Start(0));
    // Diagnostics are untrusted and may include a very large input line.
    let _ = stderr.take(64 * 1024).read_to_end(&mut bytes);
    let message = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_owned))
        .or_else(|| {
            Some(format!(
                "worker failed ({status}); diagnostics unavailable or exceeded 64 KiB"
            ))
        });
    Some((class, message))
}

fn parallel(cli: &Cli, resources: &[String], plans: &[BatchOutputs]) -> (usize, Vec<Failure>) {
    let mut active: Vec<Worker> = Vec::new();
    let mut finished = std::collections::BTreeMap::new();
    let mut failures = Vec::new();
    let (mut next, mut replay) = (0, 0);
    let mut stopped = false;
    loop {
        // Poll every active child before scheduling, including later inputs.
        let mut i = 0;
        while i < active.len() {
            let status = match active[i].child.try_wait() {
                Ok(Some(status)) => Some(Ok(status)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            };
            let Some(status) = status else {
                i += 1;
                continue;
            };
            let mut worker = active.swap_remove(i);
            let failure = match status {
                Ok(status) => child_failure(status, &mut worker.stderr),
                Err(error) => {
                    let _ = worker.child.kill();
                    let _ = worker.child.wait();
                    Some((
                        ExitClass::Input,
                        Some(format!("cannot wait for worker: {error}")),
                    ))
                }
            };
            if let Some((class, message)) = failure {
                failures.push((resources[worker.index].clone(), class, message));
                stopped |= !cli.continue_on_error;
            }
            finished.insert(worker.index, Some(worker.stdout));
        }
        while let Some(output) = finished.remove(&replay) {
            if let Some(mut output) = output {
                let result = output
                    .seek(SeekFrom::Start(0))
                    .and_then(|_| std::io::copy(&mut output, &mut std::io::stdout().lock()));
                if let Err(error) = result {
                    if !failures
                        .iter()
                        .any(|(input, _, _)| input == &resources[replay])
                    {
                        failures.push((
                            resources[replay].clone(),
                            ExitClass::Input,
                            Some(format!("cannot replay worker output: {error}")),
                        ));
                    }
                    stopped |= !cli.continue_on_error;
                }
            }
            replay += 1;
        }
        while !stopped && next < resources.len() && next - replay < cli.jobs {
            match spawn(cli, next, &resources[next], &plans[next]) {
                Ok(worker) => active.push(worker),
                Err(error) => {
                    failures.push((
                        resources[next].clone(),
                        ExitClass::Input,
                        Some(format!("cannot start worker: {error}")),
                    ));
                    finished.insert(next, None);
                    stopped |= !cli.continue_on_error;
                }
            }
            next += 1;
        }
        if active.is_empty() && finished.is_empty() && (stopped || next == resources.len()) {
            break;
        }
        if !active.is_empty() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    failures.sort_by(|a, b| a.0.cmp(&b.0));
    (next, failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn destination_keys_fold_case_for_absent_output_names() {
        let root = tempfile::tempdir().unwrap();
        let lower = root.path().join("out.html");
        let upper = root.path().join("OUT.HTML");
        assert_eq!(
            destination_key(&lower.to_string_lossy()),
            destination_key(&upper.to_string_lossy())
        );
    }

    #[test]
    fn worker_environment_preserves_terminal_rendering_context() {
        for terminal in [false, true] {
            let mut command = Command::new("rich");
            worker_terminal(&mut command, terminal, 117);
            let env: std::collections::BTreeMap<_, _> = command
                .get_envs()
                .map(|(key, value)| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.unwrap().to_string_lossy().into_owned(),
                    )
                })
                .collect();
            assert_eq!(
                env["RS_RICH_BATCH_TERMINAL"],
                if terminal { "1" } else { "0" }
            );
            assert_eq!(env["COLUMNS"], "117");
        }
    }
}
