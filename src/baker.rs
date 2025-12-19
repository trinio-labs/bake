use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::{broadcast, Semaphore};

use anyhow::bail;
use console::{style, Color};
use indicatif::{MultiProgress, ProgressBar};
use log::debug;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::{ChildStderr, ChildStdout},
    task::JoinSet,
};

use crate::{
    cache::{Cache, CacheResult},
    execution_plan,
    project::{config::ToolConfig, hashing::RecipeHasher, BakeProject, Recipe, RunStatus, Status},
};

/// Bakes a project by running all recipes and their dependencies.
///
/// # Arguments
/// * `project` - An `Arc` wrapped `BakeProject` instance containing project configuration and recipes.
/// * `cache` - A `Cache` instance for recipe execution caching.
/// * `execution_plan` - A pre-computed execution plan containing the recipes to execute in dependency order.
/// * `show_plan` - Whether to show the full execution plan details.
///
pub async fn bake(
    project: Arc<BakeProject>,
    cache: Cache,
    execution_plan: Vec<Vec<Recipe>>,
    show_plan: bool,
) -> anyhow::Result<()> {
    // Create .bake directories
    project.create_project_bake_dirs()?;

    if execution_plan.is_empty() {
        println!("No recipes to bake in the project.");
        return Ok(());
    }

    // Display execution plan based on flags
    if show_plan {
        execution_plan::display_full_execution_plan(&execution_plan)?;
    }

    // Display parallel execution settings in verbose mode
    if project.config.verbose {
        let available_parallelism = std::thread::available_parallelism().unwrap().get();
        let effective_max_parallel = project.config.effective_max_parallel();
        println!(
            "🔧 Parallel Execution: {} threads (system: {}, reserved: {}, effective: {})",
            effective_max_parallel,
            available_parallelism,
            project.config.reserved_threads,
            effective_max_parallel
        );
    }

    // Compute action keys (hashes) for all recipes upfront
    let mut hasher = RecipeHasher::new(&project);
    let mut recipe_hashes = BTreeMap::new();
    for level in &execution_plan {
        for recipe in level {
            let recipe_fqn = recipe.full_name();
            let hash = hasher.hash_for(&recipe_fqn)?;
            recipe_hashes.insert(recipe_fqn, hash);
        }
    }
    let recipe_hashes = Arc::new(recipe_hashes);

    let arc_cache = Arc::new(cache);
    let multi_progress = Arc::new(MultiProgress::new());
    let execution_results: Arc<Mutex<BTreeMap<String, RunStatus>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    let (cancel_tx, _) = broadcast::channel(1); // Used for Ctrl+C and fast_fail signalling.
    let mut overall_success = true;

    for (level_idx, level_recipes) in execution_plan.into_iter().enumerate() {
        if level_recipes.is_empty() {
            continue;
        }
        debug!(
            "Baking level {}: {} recipes",
            level_idx,
            level_recipes.len()
        );

        let mut level_join_set = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(project.config.effective_max_parallel()));

        for recipe_to_run in level_recipes {
            let arc_project_clone = project.clone();
            let arc_cache_clone = arc_cache.clone();
            let multi_progress_clone = multi_progress.clone();
            let results_clone = execution_results.clone();
            let semaphore_clone = semaphore.clone();
            let recipe_hashes_clone = recipe_hashes.clone();

            // Clone the sender for this specific task.
            // The task will use this cloned sender to create its own receivers.
            let cancel_tx_clone_for_task = cancel_tx.clone();
            let mut task_outer_cancel_rx = cancel_tx_clone_for_task.subscribe();

            level_join_set.spawn(async move {
                let recipe_fqn = recipe_to_run.full_name();
                let permit = match semaphore_clone.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        let status = RunStatus {
                            status: Status::Error,
                            output: "Semaphore closed".to_string(),
                        };
                        results_clone
                            .lock()
                            .unwrap()
                            .insert(recipe_fqn, status.clone());
                        return status;
                    }
                };

                // Create the progress bar option for this task.
                let progress_bar_owner = if !arc_project_clone.config.verbose {
                    Some(
                        multi_progress_clone.add(
                            ProgressBar::new_spinner()
                                .with_message(format!("Baking recipe {recipe_fqn}...")),
                        ),
                    )
                } else {
                    None
                };
                // Clone it for manage_single_recipe_execution if it will be moved there.
                let progress_bar_for_manage = progress_bar_owner.clone();

                // Listen for cancellation during manage_single_recipe_execution
                let final_status = tokio::select! {
                    biased;
                    _ = task_outer_cancel_rx.recv() => { // Use the task-specific receiver
                        if let Some(pb) = progress_bar_owner.as_ref() { // Borrow the original owner
                            pb.finish_with_message(format!(
                                "Baking recipe {}... {} (cancelled by signal)",
                                recipe_fqn,
                                style("∅").yellow()
                            ));
                        }
                        RunStatus {
                            status: Status::Error,
                            output: "Cancelled by signal (e.g. Ctrl+C or fast_fail)".to_string(),
                        }
                    }
                    // Pass the cloned progress_bar to manage_single_recipe_execution.
                    status = manage_single_recipe_execution(
                        recipe_to_run.clone(),
                        arc_project_clone,
                        arc_cache_clone,
                        recipe_hashes_clone,
                        progress_bar_for_manage, // Pass the clone that can be moved
                        cancel_tx_clone_for_task.subscribe(), // Use cloned sender to subscribe
                    ) => status,
                };

                results_clone
                    .lock()
                    .unwrap()
                    .insert(recipe_fqn.clone(), final_status.clone());
                drop(permit);
                final_status
            });
        }

        // Level processing loop: Simplified select! and centralized fast-fail logic.
        let mut level_failed_due_to_error_or_panic = false;
        while let Some(join_result) = tokio::select! {
            biased;
            // Prefer checking for Ctrl+C first only if not already fast-failing.
            _ = tokio::signal::ctrl_c(), if !level_failed_due_to_error_or_panic || !project.config.fast_fail => {
                println!("Ctrl+C received, attempting to shut down gracefully...");
                cancel_tx.send(()).ok(); // Signal all tasks to cancel.
                level_join_set.abort_all(); // Abort all tasks in the current level.
                // Drain the join set to allow tasks to clean up.
                debug!("Draining tasks after Ctrl+C for level {level_idx}...");
                while level_join_set.join_next().await.is_some() {}
                debug!("Finished draining tasks after Ctrl+C for level {level_idx}.");
                bail!("Bake process cancelled by user.");
            },
            // Then, process the next completed task.
            res = level_join_set.join_next() => res,
        } {
            match join_result {
                Ok(recipe_final_status) => {
                    debug!("Recipe finished: {recipe_final_status:?}");
                    if recipe_final_status.status == Status::Error {
                        overall_success = false;
                        level_failed_due_to_error_or_panic = true;
                        if project.config.fast_fail {
                            cancel_tx.send(()).ok(); // Signal other tasks.
                            level_join_set.abort_all(); // Abort remaining tasks in this level.
                                                        // No immediate bail here; drain and then bail after the loop.
                        }
                    }
                }
                Err(join_err) => {
                    debug!("Task join error: {join_err:?}");
                    if !join_err.is_cancelled() {
                        overall_success = false;
                        level_failed_due_to_error_or_panic = true;
                        eprintln!("A baking task panicked: {join_err}");
                        if project.config.fast_fail {
                            cancel_tx.send(()).ok();
                            level_join_set.abort_all();
                            // No immediate bail here; drain and then bail after the loop.
                        }
                    }
                }
            }
            // If fast_fail is enabled and an error/panic occurred, break to drain and then bail.
            if level_failed_due_to_error_or_panic && project.config.fast_fail {
                break;
            }
        }
        debug!("Finished processing level {level_idx} tasks. Draining any remaining...");
        // Drain any remaining tasks (e.g., if fast_fail broke the loop or all tasks completed normally)
        while level_join_set.join_next().await.is_some() {}
        debug!("All tasks for level {level_idx} drained.");

        // Centralized fast-fail bail for the current level.
        if project.config.fast_fail && !overall_success {
            handle_fast_fail_for_level(&execution_results, level_idx)?;
        }
    }

    // Final error reporting based on execution_results.
    process_final_results(&execution_results, overall_success)?;

    Ok(())
}

/// Handles fast-fail logic for a specific level
fn handle_fast_fail_for_level(
    execution_results: &Arc<Mutex<BTreeMap<String, RunStatus>>>,
    level_idx: usize,
) -> anyhow::Result<()> {
    let errors = execution_results
        .lock()
        .map_err(|e| anyhow::anyhow!("Failed to acquire lock on execution results: {}", e))?;
    let failed_recipe_msgs: Vec<String> = errors
        .iter()
        .filter(|(_, status)| status.status == Status::Error)
        .map(|(fqn, status)| {
            format!(
                "  - Recipe '{}': {}",
                fqn,
                status.output.lines().next().unwrap_or("failed")
            )
        })
        .collect();

    if !failed_recipe_msgs.is_empty() {
        bail!(
            "Fast fail triggered due to error(s) in level {}:
{}
Aborting bake.",
            level_idx,
            failed_recipe_msgs.join("\n")
        );
    } else {
        // This case might happen if a panic occurred that wasn't directly tied to a recipe result,
        // or if a cancellation signal was processed before any recipe error.
        bail!("Fast fail triggered in level {}, aborting bake.", level_idx);
    }
}

/// Processes final results and reports errors
fn process_final_results(
    execution_results: &Arc<Mutex<BTreeMap<String, RunStatus>>>,
    overall_success: bool,
) -> anyhow::Result<()> {
    if !overall_success {
        let locked_results = execution_results
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock on execution results: {}", e))?;
        let final_errors: Vec<String> = locked_results
            .iter()
            .filter_map(|(fqn, status_obj)| {
                if status_obj.status == Status::Error {
                    Some(format!(
                        "  - Recipe '{}' failed: {}",
                        fqn,
                        status_obj.output.trim_end_matches('\n')
                    ))
                } else {
                    None
                }
            })
            .collect();

        if !final_errors.is_empty() {
            bail!(
                "Bake completed with errors:
{}",
                final_errors.join("\n")
            );
        } else {
            // This case can occur if overall_success is false due to a cancellation or panic
            // not directly tied to a specific recipe's error output, or if fast_fail bailed early
            // but somehow didn't produce specific error messages above.
            bail!("Bake process failed or was cancelled without specific recipe errors reported.");
        }
    }
    Ok(())
}

/// Manages the execution of a single recipe, including caching, running, progress, and cancellation.
async fn manage_single_recipe_execution(
    recipe_to_run: Recipe,
    project: Arc<BakeProject>,
    cache: Arc<Cache>,
    recipe_hashes: Arc<BTreeMap<String, String>>,
    progress_bar: Option<ProgressBar>,
    mut cancel_rx: broadcast::Receiver<()>, // Receiver for cancellation signals
) -> RunStatus {
    let recipe_fqn = recipe_to_run.full_name();
    let mut run_status = RunStatus {
        status: Status::Idle,
        output: String::new(),
    };

    tokio::select! {
        biased; // Prioritize cancellation check
        _ = cancel_rx.recv() => {
            if let Some(pb) = progress_bar.as_ref() {
                pb.finish_with_message(format!(
                    "Baking recipe {}... {} (cancelled)",
                    recipe_fqn,
                    style("∅").yellow()
                ));
            }
            run_status.status = Status::Error;
            run_status.output = "Cancelled by user or fast_fail".to_string();
        }
        _ = async {
            // Check cache for the recipe.
            if recipe_to_run.cache.is_some() {
                // Get the action key (hash) for this recipe
                let action_key = recipe_hashes
                    .get(&recipe_fqn)
                    .expect("Recipe hash should have been computed");

                match cache.get(action_key, &recipe_fqn).await {
                    Ok(CacheResult::Hit { stdout: _, stderr: _, exit_code: _ }) => {
                        if let Some(pb) = progress_bar.as_ref() {
                            pb.finish_with_message(format!(
                                "Baking recipe {}... {} (cached)",
                                recipe_fqn,
                                console::style("✓").green()
                            ));
                        } else if project.config.verbose {
                            println!("{}: {} (cached)", recipe_fqn, console::style("✓").green());
                        }
                        run_status.status = Status::Done;
                        return; // Return from the async block, not the whole function
                    }
                    Ok(CacheResult::Miss) => {
                        debug!("Cache miss for recipe: {recipe_fqn}. Proceeding with execution.");
                        // If it's a miss, we simply proceed to run the recipe normally.
                    }
                    Err(e) => {
                        debug!("Cache check error for recipe {}: {}. Proceeding with execution.", recipe_fqn, e);
                        // On cache error, proceed with execution
                    }
                }
            }

            // If not cached (i.e., CacheResult::Miss was matched and fell through) or cache is disabled, run the recipe.
            run_status.status = Status::Running;
            if let Some(pb) = progress_bar.as_ref() {
                pb.set_message(format!("Baking recipe {recipe_fqn}... (running)"));
            }
            match run_recipe(
                &recipe_to_run,
                project.get_recipe_log_path(&recipe_fqn),
                &project.config
            ).await {
                Ok(result) => {
                    run_status.status = Status::Done;
                    if recipe_to_run.cache.is_some() { // Try to cache if successful run
                        // Get the action key (hash) for this recipe
                        let action_key = recipe_hashes
                            .get(&recipe_fqn)
                            .expect("Recipe hash should have been computed");

                        // Collect output paths from recipe cache configuration
                        let output_paths: Vec<PathBuf> = if let Some(ref cache_config) = recipe_to_run.cache {
                            cache_config
                                .outputs
                                .iter()
                                .map(|output| {
                                    // Resolve output path relative to cookbook directory
                                    recipe_to_run
                                        .config_path
                                        .parent()
                                        .unwrap()
                                        .join(output)
                                })
                                .collect()
                        } else {
                            vec![]
                        };

                        if let Err(e) = cache.put(action_key, &recipe_fqn, &output_paths, &result.stdout, &result.stderr, result.exit_code).await {
                            let err_msg = format!("Cache store error for {recipe_fqn}: {e}");
                            if let Some(pb) = progress_bar.as_ref() { pb.println(&err_msg); } else { println!("{err_msg}"); }
                        }
                    }
                    if let Some(pb) = progress_bar.as_ref() {
                        pb.finish_with_message(format!(
                            "Baking recipe {}... {}",
                            recipe_fqn,
                            console::style("✓").green()
                        ));
                    }
                }
                Err(e) => {
                    run_status.status = Status::Error;
                    run_status.output = e; // e is already a String, no need to clone
                    if let Some(pb) = progress_bar.as_ref() {
                        pb.finish_with_message(format!(
                            "Baking recipe {}... {}",
                            recipe_fqn,
                            console::style("✗").red()
                        ));
                    }
                }
            }
        } => {}
    }
    run_status
}

/// Runs a single recipe as a system process and handles its output.
///
/// # Arguments
/// * `recipe` - The `Recipe` to run.
/// * `log_file_path` - The `PathBuf` where the recipe's output log should be stored.
/// * `config` - The `ToolConfig` containing settings like verbosity and environment cleaning.
///
/// Result of running a recipe
pub struct RecipeRunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub async fn run_recipe(
    recipe: &Recipe,
    log_file_path: PathBuf,
    config: &ToolConfig,
) -> Result<RecipeRunResult, String> {
    debug!("Running recipe: {}", recipe.full_name());
    let env_values: Vec<(String, String)> = recipe
        .environment
        .iter()
        .map(|name| (name.clone(), std::env::var(name).unwrap_or_default()))
        .collect();

    let mut cmd = tokio::process::Command::new("sh");
    let run_cmd = if config.clean_environment {
        cmd.env_clear().envs(env_values)
    } else {
        &mut cmd
    };

    debug!("Spawning command for recipe: {}", recipe.full_name());
    let start_time = Instant::now();
    if config.verbose {
        println_recipe(
            "============== Started baking recipe ==============",
            &recipe.full_name(),
        )
    }
    let result = run_cmd
        .current_dir(recipe.config_path.parent().ok_or_else(|| {
            format!(
                "Recipe config path '{}' has no parent directory",
                recipe.config_path.display()
            )
        })?)
        .arg("-c")
        .arg(format!("set -e; {}", recipe.run.clone()))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    debug!("Process finished for recipe: {}", recipe.full_name());
    match result {
        Ok(mut child) => {
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let process_handle = tokio::spawn(process_output(
                stdout,
                stderr,
                recipe.full_name(),
                log_file_path,
                config.verbose,
            ));
            let exit_status = child.wait().await.map_err(|e| {
                format!(
                    "Failed to wait for recipe '{}': {}",
                    recipe.full_name(),
                    e
                )
            })?;
            let exit_code = exit_status.code().unwrap_or(-1);

            let (stdout_str, stderr_str) = process_handle.await.map_err(|e| {
                format!(
                    "Error processing output for recipe '{}': {}",
                    recipe.full_name(),
                    e
                )
            })??;

            let elapsed = start_time.elapsed();
            if config.verbose {
                println_recipe(
                    &format!("============== Finished baking recipe ({elapsed:.2?}) ============="),
                    &recipe.full_name(),
                )
            }

            if !exit_status.success() {
                return Err(format!(
                    "Recipe {} failed with exit code {}",
                    recipe.full_name(),
                    exit_code
                ));
            }

            Ok(RecipeRunResult {
                stdout: stdout_str,
                stderr: stderr_str,
                exit_code,
            })
        }
        Err(err) => {
            return Err(format!(
                "Failed to spawn command for recipe '{}': {}",
                recipe.full_name(),
                err
            ));
        }
    }
}

/// Generates a terminal color based on a string hash.
/// Avoids very dark and very bright colors for better readability.
fn name_to_term_color(string: &str) -> Color {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    string.hash(&mut hasher);
    let hash = hasher.finish();
    let mut color_num = hash % 229;

    // Remove dark and bright colors
    color_num = match color_num {
        0 => 1,
        15..=24 => color_num - 14,
        52..=56 => color_num - 51,
        _ => color_num,
    };

    Color::Color256(color_num as u8)
}

/// Processes the output (stdout and stderr) of a spawned command.
///
/// It collects stdout and stderr separately, optionally prints them to the console if `verbose` is true,
/// and writes the combined output to the specified `log_file_path`.
///
/// # Arguments
/// * `stdout` - The `ChildStdout` stream of the spawned process.
/// * `stderr` - The `ChildStderr` stream of the spawned process.
/// * `recipe_name` - The name of the recipe, used for prefixing verbose output.
/// * `log_file_path` - The `PathBuf` where the combined output log will be written.
/// * `verbose` - A boolean indicating whether to print each line of output to the console.
///
/// # Returns
/// A tuple of (stdout_string, stderr_string)
///
async fn process_output(
    stdout: ChildStdout,
    stderr: ChildStderr,
    recipe_name: String,
    log_file_path: PathBuf,
    verbose: bool,
) -> Result<(String, String), String> {
    let mut join_set = JoinSet::new();
    let stdout_str = Arc::new(Mutex::new(String::new()));
    let stderr_str = Arc::new(Mutex::new(String::new()));

    /// Helper to read lines from a stream, print if verbose, and append to a shared string.
    async fn collect_output<T: AsyncRead + Unpin>(
        output: T,
        recipe_name: String,
        output_string: Arc<Mutex<String>>,
        verbose: bool,
    ) {
        let mut reader = BufReader::new(output).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if verbose {
                println_recipe(&line, &recipe_name);
            }
            if let Ok(mut output) = output_string.lock() {
                output.push_str(&(line + "\n"));
            }
        }
    }

    join_set.spawn(collect_output(
        stdout,
        recipe_name.clone(),
        stdout_str.clone(),
        verbose,
    ));

    join_set.spawn(collect_output(
        stderr,
        recipe_name.clone(),
        stderr_str.clone(),
        verbose,
    ));

    while (join_set.join_next().await).is_some() {}

    // Get the collected strings
    let stdout_string = stdout_str
        .lock()
        .map_err(|e| format!("Failed to acquire lock on stdout string: {e}"))?
        .clone();
    let stderr_string = stderr_str
        .lock()
        .map_err(|e| format!("Failed to acquire lock on stderr string: {e}"))?
        .clone();

    // Write combined output to log file
    let combined_output = format!("{}{}", stdout_string, stderr_string);
    match tokio::fs::File::create(log_file_path.clone()).await {
        Ok(mut file) => {
            if let Err(err) = file.write_all(combined_output.as_bytes()).await {
                return Err(format!(
                    "Failed to write to log file for recipe '{}' at '{}': {}",
                    recipe_name,
                    log_file_path.display(),
                    err
                ));
            };
        }
        Err(err) => {
            return Err(format!(
                "Failed to create log file for recipe '{}' at '{}': {}",
                recipe_name,
                log_file_path.display(),
                err
            ));
        }
    }

    Ok((stdout_string, stderr_string))
}

/// Prints a line to the console, prefixed with a colored recipe name.
fn println_recipe(line: &str, recipe_name: &str) {
    let color = name_to_term_color(recipe_name);
    let formatted_line = format!("[{}]: {}", style(&recipe_name).fg(color), line);
    println!("{formatted_line}");
}

// Integration tests have been moved to tests/integration/baker_tests.rs
