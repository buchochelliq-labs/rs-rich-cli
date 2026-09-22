//! CLI-only batch planning and bounded process execution.
//!
//! Fail-fast stops scheduling as soon as any active worker fails; workers already
//! started finish, and their terminal output is replayed in resource order.
use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub(super) fn install_interrupt_handler() -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::Relaxed))
}

fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

pub(super) fn check_interrupted() -> Result<(), String> {
    if interrupted() {
        Err("batch interrupted".into())
    } else {
        Ok(())
    }
}

fn progress(cli: &Cli, completed: usize, failed: usize, total: usize) {
    if cli.progress
        && !cli.dry_run
        && cli.report_format == ReportFormat::Human
        && std::io::stderr().is_terminal()
    {
        eprintln!("Batch: {completed} completed, {failed} failed, {total} total");
    }
}

fn interrupted_report(
    cli: &Cli,
    planned: usize,
    attempted: usize,
    settled: usize,
    failures: &[Failure],
) -> ExitCode {
    if cli.report_format == ReportFormat::Json {
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": false, "code": "interrupted", "exit_code": 130,
                "result": {"planned": planned, "attempted": attempted,
                    "completed": settled - failures.len(), "failed": failures.len(),
                    "interrupted": attempted - settled, "skipped": planned - attempted,
                    "failures": failure_json(failures)}
            })
        );
    } else {
        eprintln!("rich: batch interrupted");
    }
    ExitCode::from(130)
}

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

struct OutputRoots {
    html: Option<batch_output::OutputRoot>,
    svg: Option<batch_output::OutputRoot>,
}

impl OutputRoots {
    fn capture(cli: &Cli, total: usize) -> std::io::Result<Self> {
        let capture = |configured: Option<&str>| {
            configured
                .map(|path| {
                    let path = Path::new(path);
                    let directory_mode =
                        cli.batch_paths.preserve_dirs || cli.batch_paths.template.is_some();
                    let root = if directory_mode || (total > 1 && path.is_dir()) {
                        path
                    } else {
                        path.parent().unwrap_or_else(|| Path::new("."))
                    };
                    batch_output::OutputRoot::capture(root)
                })
                .transpose()
        };
        Ok(Self {
            html: capture(cli.export_html.as_deref())?,
            svg: capture(cli.export_svg.as_deref())?,
        })
    }

    fn prepare(
        &self,
        outputs: &BatchOutputs,
        create: bool,
    ) -> std::io::Result<Vec<(&'static str, batch_output::Destination)>> {
        let mut destinations = Vec::new();
        for (kind, root, path) in [
            ("html", &self.html, &outputs.html),
            ("svg", &self.svg, &outputs.svg),
        ] {
            if let (Some(root), Some(path)) = (root, path) {
                destinations.push((kind, root.destination(Path::new(path), create)?));
            }
        }
        Ok(destinations)
    }
}

pub(super) fn run_batch(cli: &Cli) -> ExitCode {
    if interrupted() {
        return interrupted_report(cli, cli.resources.len(), 0, 0, &[]);
    }
    let resources = match expand_batch_resources(&cli.resources) {
        Ok(resources) => resources,
        Err(message) => {
            if interrupted() {
                return interrupted_report(cli, cli.resources.len(), 0, 0, &[]);
            }
            return fail(cli, ExitClass::Input, message);
        }
    };
    let roots = match OutputRoots::capture(cli, resources.len()) {
        Ok(roots) => roots,
        Err(error) => {
            return fail(
                cli,
                ExitClass::Input,
                format!("cannot acquire export directory: {error}"),
            )
        }
    };
    let mut taken = std::collections::BTreeSet::new();
    let mut keys = std::collections::BTreeSet::new();
    let mut existing_outputs: Vec<String> = Vec::new();
    let mut input_keys = std::collections::BTreeSet::new();
    for input in &resources {
        if interrupted() {
            return interrupted_report(cli, resources.len(), 0, 0, &[]);
        }
        input_keys.insert(destination_key(input));
    }
    let mut planned_directories = std::collections::BTreeSet::new();
    let mut plans = Vec::with_capacity(resources.len());
    let mut errors = Vec::new();
    for (index, input) in resources.iter().enumerate() {
        if interrupted() {
            return interrupted_report(cli, resources.len(), 0, 0, &[]);
        }
        let mut outputs = BatchOutputs::default();
        for (configured, extension, slot) in [
            (cli.export_html.as_deref(), "html", &mut outputs.html),
            (cli.export_svg.as_deref(), "svg", &mut outputs.svg),
        ] {
            if interrupted() {
                return interrupted_report(cli, resources.len(), 0, 0, &[]);
            }
            let Some(configured) = configured else {
                continue;
            };
            let candidate = if cli.batch_paths.preserve_dirs || cli.batch_paths.template.is_some() {
                if is_url(input) {
                    errors.push((
                        input.clone(),
                        ExitClass::Usage,
                        Some("batch path options require local inputs".into()),
                    ));
                    continue;
                }
                match batch_paths::plan_destination(
                    Path::new(input),
                    Path::new(configured),
                    extension,
                    index + 1,
                    &cli.batch_paths,
                ) {
                    Ok(plan) => {
                        if cli.batch_paths.preserve_dirs {
                            planned_directories.extend(plan.create_parents);
                        }
                        let Some(path) = plan.path.to_str() else {
                            errors.push((
                                input.clone(),
                                ExitClass::Usage,
                                Some("CLI output path is not UTF-8".into()),
                            ));
                            continue;
                        };
                        path.to_owned()
                    }
                    Err(error) => {
                        errors.push((input.clone(), ExitClass::Usage, Some(error.to_string())));
                        continue;
                    }
                }
            } else {
                batch_export_path(Some(configured), input, index, resources.len())
                    .expect("configured export")
            };
            *slot = Some(candidate.clone());
            match resolve_destination(cli, candidate, &mut taken) {
                Ok(path) => {
                    let key = destination_key(&path);
                    if input_keys.contains(&key)
                        || resources
                            .iter()
                            .take_while(|_| !interrupted())
                            .any(|input| same_file::is_same_file(input, &path).unwrap_or(false))
                    {
                        errors.push((
                            input.clone(),
                            ExitClass::Usage,
                            Some(format!(
                                "batch output would overwrite a batch input: {path}"
                            )),
                        ));
                    }
                    if interrupted() {
                        return interrupted_report(cli, resources.len(), 0, 0, &[]);
                    }
                    let hard_link_collision = existing_outputs
                        .iter()
                        .take_while(|_| !interrupted())
                        .any(|output| same_file::is_same_file(output, &path).unwrap_or(false));
                    if interrupted() {
                        return interrupted_report(cli, resources.len(), 0, 0, &[]);
                    }
                    existing_outputs.push(path.clone());
                    if (!keys.insert(key) || hard_link_collision)
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
                            if !parent.is_dir() && !cli.batch_paths.preserve_dirs {
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
    if interrupted() {
        return interrupted_report(cli, resources.len(), 0, 0, &[]);
    }
    if cli.dry_run {
        let class = failure_class(&errors);
        if cli.report_format == ReportFormat::Json {
            let mut report = serde_json::json!({
                "ok": errors.is_empty(), "code": class.name(), "exit_code": class.code(),
                "result": { "dry_run": true, "planned": resources.len(), "attempted": 0,
                    "resources": resources.iter().zip(&plans).map(|(input, out)| serde_json::json!({
                        "resource": input, "html": out.html, "svg": out.svg,
                    })).collect::<Vec<_>>(), "errors": failure_json(&errors) }
            });
            if cli.batch_paths.preserve_dirs {
                report["result"]["directories"] = serde_json::json!(planned_directories);
            }
            eprintln!("{report}");
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
            for directory in &planned_directories {
                println!("  Create directory: {}", directory.display());
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
    for outputs in &plans {
        if let Err(error) = roots.prepare(outputs, cli.batch_paths.preserve_dirs) {
            return fail(
                cli,
                ExitClass::Input,
                format!("cannot prepare export directory: {error}"),
            );
        }
    }
    progress(cli, 0, 0, resources.len());
    let (attempted, settled, failures) = execute(cli, &resources, &plans, &roots);
    if interrupted() {
        return interrupted_report(cli, resources.len(), attempted, settled, &failures);
    }
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
    stdout: Option<std::fs::File>,
    stderr: std::fs::File,
    exports: Vec<(batch_output::Destination, PathBuf)>,
    _staging: tempfile::TempDir,
}

// Every exit path, including cancellation and replay errors, reaps started workers.
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Spooling changes the child's physical stdout, so forward the parent's
// rendering context separately. Paging continues to inspect the real TTY.
fn worker_terminal(command: &mut Command, terminal: bool, width: usize) {
    command.env("RS_RICH_BATCH_TERMINAL", if terminal { "1" } else { "0" });
    command.env("COLUMNS", width.to_string());
}

fn spawn(
    cli: &Cli,
    index: usize,
    input: &str,
    outputs: &BatchOutputs,
    resources: &[String],
    roots: &OutputRoots,
) -> std::io::Result<Worker> {
    for path in [outputs.html.as_deref(), outputs.svg.as_deref()]
        .into_iter()
        .flatten()
    {
        if resources.iter().any(|input| {
            destination_key(input) == destination_key(path)
                || same_file::is_same_file(input, path).unwrap_or(false)
        }) {
            return Err(std::io::Error::other(
                "batch output would overwrite a batch input",
            ));
        }
        if cli.batch_paths.preserve_dirs || cli.batch_paths.template.is_some() {
            for root in [cli.export_html.as_deref(), cli.export_svg.as_deref()]
                .into_iter()
                .flatten()
            {
                if Path::new(path).starts_with(root)
                    && !destination_key(path).starts_with(destination_key(root))
                {
                    return Err(std::io::Error::other("destination is outside output root"));
                }
            }
        }
    }
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let mut command = Command::new(std::env::current_exe()?);
    worker_terminal(
        &mut command,
        std::io::stdout().is_terminal(),
        Console::new().width(),
    );
    command.args(&cli.worker_args).args(["--report", "json"]);
    let staging = tempfile::tempdir()?;
    let mut exports = Vec::new();
    for (kind, destination) in roots.prepare(outputs, false)? {
        let path = staging.path().join(format!("export.{kind}"));
        command.arg(format!("--export-{kind}")).arg(&path);
        exports.push((destination, path));
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
        stdout: Some(stdout),
        stderr,
        exports,
        _staging: staging,
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

// Keep a blocked pipe consumer from preventing SIGINT cleanup. The copy thread
// owns a duplicate descriptor, not Rust's global stdout lock (which shutdown
// must acquire). On cancellation the CLI exits after workers have been reaped;
// on every normal path we join the copy thread before replaying the next item.
fn replay_output(mut output: std::fs::File) -> std::io::Result<()> {
    #[cfg(unix)]
    let mut destination = {
        use std::os::fd::AsFd;
        std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned()?)
    };
    #[cfg(windows)]
    let mut destination: Box<dyn std::io::Write + Send> = {
        use std::os::windows::io::AsHandle;
        // Rust's console writer transcodes UTF-8 to UTF-16 for WriteConsoleW.
        // Raw handles are appropriate only for redirected stdout on Windows.
        if std::io::stdout().is_terminal() {
            Box::new(std::io::stdout())
        } else {
            Box::new(std::fs::File::from(
                std::io::stdout().as_handle().try_clone_to_owned()?,
            ))
        }
    };
    #[cfg(not(any(unix, windows)))]
    let mut destination = std::io::stdout();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("batch-replay".into())
        .spawn(move || {
            let result = output
                .seek(SeekFrom::Start(0))
                .and_then(|_| std::io::copy(&mut output, &mut destination))
                .map(|_| ());
            let _ = sender.send(result);
        })?;
    loop {
        if interrupted() {
            return Ok(());
        }
        match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(result) => {
                let _ = thread.join();
                return result;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
                return Err(std::io::Error::other("worker output replay thread failed"));
            }
        }
    }
}

fn execute(
    cli: &Cli,
    resources: &[String],
    plans: &[BatchOutputs],
    roots: &OutputRoots,
) -> (usize, usize, Vec<Failure>) {
    let mut active: Vec<Worker> = Vec::new();
    let mut finished = std::collections::BTreeMap::new();
    let mut failures = Vec::new();
    let (mut next, mut replay) = (0, 0);
    let mut stopped = false;
    let mut settled = 0;
    loop {
        if interrupted() {
            break;
        }
        let previous_settled = settled;
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
            let mut failure = match status {
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
            if failure.is_none() && !interrupted() {
                for (destination, path) in &worker.exports {
                    let result = std::fs::File::open(path).and_then(|mut source| {
                        destination.publish(
                            &mut source,
                            cli.overwrite || cli.collision == CollisionPolicy::Overwrite,
                        )
                    });
                    if let Err(error) = result {
                        failure = Some((
                            ExitClass::Input,
                            Some(format!("cannot publish batch export: {error}")),
                        ));
                        break;
                    }
                }
            }
            if let Some((class, message)) = failure {
                failures.push((resources[worker.index].clone(), class, message));
                stopped |= !cli.continue_on_error;
            }
            settled += 1;
            finished.insert(worker.index, worker.stdout.take());
        }
        if settled != previous_settled {
            progress(
                cli,
                settled - failures.len(),
                failures.len(),
                resources.len(),
            );
        }
        while !interrupted() {
            let Some(output) = finished.remove(&replay) else {
                break;
            };
            if let Some(output) = output {
                let result = replay_output(output);
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
        while !interrupted() && !stopped && next < resources.len() && next - replay < cli.jobs {
            match spawn(cli, next, &resources[next], &plans[next], resources, roots) {
                Ok(worker) => active.push(worker),
                Err(error) => {
                    failures.push((
                        resources[next].clone(),
                        ExitClass::Input,
                        Some(format!("cannot start worker: {error}")),
                    ));
                    settled += 1;
                    progress(
                        cli,
                        settled - failures.len(),
                        failures.len(),
                        resources.len(),
                    );
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
    (next, settled, failures)
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
