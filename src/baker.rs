use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
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
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{ChildStderr, ChildStdout},
    task::JoinSet,
};

use crate::{
    cache::{Cache, CacheResult},
    project::{config::ToolConfig, BakeProject, Recipe, RunStatus, Status},
};

/// Bakes a project by running all recipes and their dependencies.
///
/// # Arguments
/// * `project` - An `Arc` wrapped `BakeProject` instance containing project configuration and recipes.
/// * `cache` - A `Cache` instance for recipe execution caching.
/// * `filter` - An optional string slice (`&str`). If provided, only recipes whose
///   fully qualified names start with this filter will be executed.
///
pub async fn bake(
    project: Arc<BakeProject>,
    cache: Cache,
    filter: Option<&str>,
) -> anyhow::Result<()> {
    // Create .bake directories
    project.create_project_bake_dirs()?;

    let execution_plan = project.get_recipes_for_execution(filter)?;
    if execution_plan.is_empty() {
        if filter.is_some() {
            println!(
                "No recipes found matching the filter: '{}'",
                filter.unwrap()
            );
        } else {
            println!("No recipes to bake in the project.");
        }
        return Ok(());
    }

    let arc_cache = Arc::new(cache);
    let multi_progress = Arc::new(MultiProgress::new());
    let execution_results: Arc<Mutex<BTreeMap<String, RunStatus>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    let (cancel_tx, _) = broadcast::channel(1); // Used for Ctrl+C and fast_fail signalling.
    #[allow(unused_assignments)] // This warning is benign due to early bail with fast_fail.
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
        // Semaphore to limit concurrency within the current execution level.
        let semaphore = Arc::new(Semaphore::new(project.config.max_parallel));

        for recipe_to_run in level_recipes {
            let arc_project_clone = project.clone();
            let arc_cache_clone = arc_cache.clone();
            let multi_progress_clone = multi_progress.clone();
            let results_clone = execution_results.clone();
            let semaphore_clone = semaphore.clone();
            let cancel_rx_clone: broadcast::Receiver<()> = cancel_tx.subscribe();

            level_join_set.spawn(async move {
                let recipe_fqn = recipe_to_run.full_name(); // Capture FQN for potential early exit

                let permit = match semaphore_clone.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        // Semaphore closed, likely due to broader cancellation
                        let status = RunStatus {
                            status: Status::Error,
                            output: "Semaphore closed".to_string(),
                        };
                        // Record the error status before returning
                        results_clone
                            .lock()
                            .unwrap()
                            .insert(recipe_fqn, status.clone());
                        return status; // Early return from the spawned task
                    }
                };

                let progress_bar: Option<ProgressBar> = if !arc_project_clone.config.verbose {
                    Some(
                        multi_progress_clone.add(
                            ProgressBar::new_spinner()
                                .with_message(format!("Baking recipe {recipe_fqn}...")),
                        ),
                    )
                } else {
                    None
                };

                let final_status = manage_single_recipe_execution(
                    recipe_to_run.clone(), // Recipe is Clone
                    arc_project_clone,
                    arc_cache_clone,
                    progress_bar, // Pass the owned Option<ProgressBar>
                    cancel_rx_clone,
                )
                .await;

                results_clone
                    .lock()
                    .unwrap()
                    .insert(recipe_fqn, final_status.clone());

                drop(permit); // Release the semaphore permit.
                final_status // Return the final status of this recipe.
            });
        }

        // Wait for all tasks in the current level to complete, or handle Ctrl+C / fast_fail.
        loop {
            debug!("Waiting for tasks in level {level_idx} to complete...");
            tokio::select! {
                biased; // Prioritize processing completed tasks and then cancellation.
                // Handle task completion or JoinSet empty
                maybe_join_result = level_join_set.join_next() => {
                    if let Some(join_result) = maybe_join_result {
                        // A task has completed.
                        match join_result {
                            Ok(recipe_final_status) => {
                                debug!("Recipe finished: {recipe_final_status:?}");
                                if recipe_final_status.status == Status::Error {
                                    overall_success = false;
                                    if project.config.fast_fail {
                                        cancel_tx.send(()).ok(); // Signal other tasks to cancel.
                                        level_join_set.abort_all(); // Abort remaining tasks in this level.
                                        // Drain remaining tasks to ensure cleanup.
                                        debug!("Draining tasks after fast_fail (recipe error) for level {level_idx}...");
                                        while level_join_set.join_next().await.is_some() {}
                                        debug!("Finished draining tasks after fast_fail (recipe error) for level {level_idx}.");
                                        bail!(
                                            "Recipe '{}' failed. Fast fail enabled, aborting...",
                                            recipe_final_status.output.lines().next().unwrap_or("unknown recipe")
                                        );
                                    }
                                }
                            }
                            Err(join_err) => {
                                debug!("Task join error: {join_err:?}");
                                // A task panicked or was cancelled by abort_all().
                                // Only treat non-cancelled panics as new errors here.
                                if !join_err.is_cancelled() {
                                    overall_success = false;
                                    eprintln!("A baking task panicked: {join_err}");
                                    if project.config.fast_fail {
                                        cancel_tx.send(()).ok();
                                        level_join_set.abort_all();
                                        debug!("Draining tasks after fast_fail (panic) for level {level_idx}...");
                                        while level_join_set.join_next().await.is_some() {}
                                        debug!("Finished draining tasks after fast_fail (panic) for level {level_idx}.");
                                        bail!("Task panicked and fast_fail is enabled. Aborting.");
                                    }
                                }
                            }
                        }
                    } else {
                        // level_join_set.join_next() returned None, so the set is empty.
                        debug!("All tasks for level {level_idx} complete (join_next returned None).");
                        break; // Exit the select loop for this level.
                    }
                }

                // Handle Ctrl+C
                _ = tokio::signal::ctrl_c() => {
                    println!("Ctrl+C received, attempting to shut down gracefully...");
                    cancel_tx.send(()).ok(); // Signal tasks to cancel.
                    level_join_set.abort_all();
                    // Drain remaining tasks to ensure cleanup after abort_all.
                    debug!("Draining tasks after Ctrl+C for level {level_idx}...");
                    while level_join_set.join_next().await.is_some() {
                        // Loop to ensure all tasks (especially cancelled ones) are joined.
                    }
                    debug!("Finished draining tasks after Ctrl+C for level {level_idx}.");
                    bail!("Bake process cancelled by user.");
                }
            }
        }

        // If fast_fail was triggered and we bailed, this part won't be reached for that level.
        if !overall_success && project.config.fast_fail {
            bail!("Fast fail triggered, aborting bake.");
        }
    }

    // Final error reporting based on execution_results.
    if !overall_success {
        let final_errors: Vec<String> = execution_results
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(fqn, status_obj)| {
                if status_obj.status == Status::Error {
                    Some(format!(
                        "Recipe '{}' failed: {}",
                        fqn,
                        status_obj.output.trim_end_matches('\n')
                    ))
                } else {
                    None
                }
            })
            .collect();

        if !final_errors.is_empty() {
            bail!("Some recipes failed to run:\n{}", final_errors.join("\n"));
        } else {
            // This case can occur if overall_success is false due to a cancellation or panic
            // not directly tied to a specific recipe's error output, or if fast_fail bailed early.
            bail!("Bake process failed or was cancelled.");
        }
    }

    Ok(())
}

/// Manages the execution of a single recipe, including caching, running, progress, and cancellation.
async fn manage_single_recipe_execution(
    recipe_to_run: Recipe,
    project: Arc<BakeProject>,
    cache: Arc<Cache>,
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
            let mut skip_run_due_to_cache = false;
            // Check cache for the recipe.
            if recipe_to_run.cache.is_some() &&
               matches!(cache.get(&recipe_fqn).await, CacheResult::Hit(_)) {
                if let Some(pb) = progress_bar.as_ref() {
                    pb.finish_with_message(format!(
                        "Baking recipe {}... {} (cached)",
                        recipe_fqn,
                        console::style("✓").green()
                    ));
                } else if project.config.verbose {
                    println!("{}: {} (cached)", recipe_fqn, console::style("✓").green());
                }
                skip_run_due_to_cache = true;
                run_status.status = Status::Done;
            }

            if !skip_run_due_to_cache {
                // If not cached or cache is disabled, run the recipe.
                run_status.status = Status::Running;
                if let Some(pb) = progress_bar.as_ref() {
                    pb.set_message(format!("Baking recipe {recipe_fqn}... (running)"));
                }
                match run_recipe(
                    &recipe_to_run,
                    project.get_recipe_log_path(&recipe_fqn),
                    &project.config
                ).await {
                    Ok(_) => {
                        run_status.status = Status::Done;
                        // If the run was successful, try to cache it if configured.
                        if recipe_to_run.cache.is_some() {
                            if let Err(e) = cache.put(&recipe_fqn).await {
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
                        run_status.output = e.clone();
                        if let Some(pb) = progress_bar.as_ref() {
                            pb.finish_with_message(format!(
                                "Baking recipe {}... {}",
                                recipe_fqn,
                                console::style("✗").red()
                            ));
                        }
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
pub async fn run_recipe(
    recipe: &Recipe,
    log_file_path: PathBuf,
    config: &ToolConfig,
) -> Result<(), String> {
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
        .current_dir(recipe.config_path.parent().unwrap())
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
            if let Ok(exit_code) = child.wait().await {
                if !exit_code.success() {
                    return Err(format!(
                        "Recipe {} failed with exit code {}",
                        recipe.full_name(),
                        exit_code
                    ));
                }
            }
            if let Err(err) = process_handle.await {
                return Err(format!("Could wait for process output thread: {err}"));
            }
        }
        Err(err) => {
            return Err(format!("Could not spawn process: {err}"));
        }
    }
    let elapsed = start_time.elapsed();
    if config.verbose {
        println_recipe(
            &format!("============== Finished baking recipe ({elapsed:.2?}) ============="),
            &recipe.full_name(),
        )
    }
    Ok(())
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
/// It collects all output lines, optionally prints them to the console if `verbose` is true,
/// and writes the complete output to the specified `log_file_path`.
///
/// # Arguments
/// * `stdout` - The `ChildStdout` stream of the spawned process.
/// * `stderr` - The `ChildStderr` stream of the spawned process.
/// * `recipe_name` - The name of the recipe, used for prefixing verbose output.
/// * `log_file_path` - The `PathBuf` where the combined output log will be written.
/// * `verbose` - A boolean indicating whether to print each line of output to the console.
///
async fn process_output(
    stdout: ChildStdout,
    stderr: ChildStderr,
    recipe_name: String,
    log_file_path: PathBuf,
    verbose: bool,
) -> Result<(), String> {
    let mut join_set = JoinSet::new();
    let output_str = Arc::new(Mutex::new(String::new()));

    /// Helper to read lines from a stream, print if verbose, and append to a shared string.
    async fn collect_output<T: AsyncRead + Unpin>(
        output: T,
        recipe_name: String,
        output_string: Arc<Mutex<String>>,
        verbose: bool,
    ) {
        let mut reader = BufReader::new(output).lines();
        while let Some(line) = reader.next_line().await.unwrap() {
            if verbose {
                println_recipe(&line, &recipe_name);
            }
            output_string.lock().unwrap().push_str(&(line + "\n"));
        }
    }

    join_set.spawn(collect_output(
        stdout,
        recipe_name.clone(),
        output_str.clone(),
        verbose,
    ));

    join_set.spawn(collect_output(
        stderr,
        recipe_name.clone(),
        output_str.clone(),
        verbose,
    ));

    while (join_set.join_next().await).is_some() {}

    match File::create(log_file_path.clone()) {
        Ok(mut file) => {
            if let Err(err) = file.write_all(output_str.lock().unwrap().as_bytes()) {
                return Err(format!(
                    "could not write log file {}: {}",
                    log_file_path.display(),
                    err
                ));
            };
        }
        Err(err) => {
            return Err(format!(
                "could not create log file {}: {}",
                log_file_path.display(),
                err
            ));
        }
    }

    Ok(())
}

/// Prints a line to the console, prefixed with a colored recipe name.
fn println_recipe(line: &str, recipe_name: &str) {
    let color = name_to_term_color(recipe_name);
    let formatted_line = format!("[{}]: {}", style(&recipe_name).fg(color), line);
    println!("{formatted_line}");
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use async_trait::async_trait;

    use crate::{
        cache::{
            Cache, CacheBuilder, CacheResult, CacheResultData, CacheStrategy, ARCHIVE_EXTENSION,
        },
        project::BakeProject,
        test_utils::TestProjectBuilder,
    };

    #[derive(Clone, Debug)]
    struct TestCacheStrategy {
        pub hit: bool,
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        async fn get(&self, _: &str) -> CacheResult {
            if self.hit {
                CacheResult::Hit(CacheResultData {
                    archive_path: PathBuf::from(format!("foo.{ARCHIVE_EXTENSION}")),
                })
            } else {
                CacheResult::Miss
            }
        }
        async fn put(&self, _: &str, _: PathBuf) -> anyhow::Result<()> {
            Ok(())
        }

        async fn from_config(_project: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
            Ok(Box::new(TestCacheStrategy { hit: false }))
        }
    }

    async fn build_cache(project: Arc<BakeProject>) -> Cache {
        CacheBuilder::new(project)
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build()
            .await
            .unwrap()
    }

    fn create_test_project() -> BakeProject {
        let mut project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build", "test"])
            .with_cookbook("bar", &["build", "test"])
            .build();

        project
            .cookbooks
            .get_mut("foo")
            .unwrap()
            .recipes
            .get_mut("build")
            .unwrap()
            .run = String::from("exit 0");
        project
            .cookbooks
            .get_mut("foo")
            .unwrap()
            .recipes
            .get_mut("test")
            .unwrap()
            .run = String::from("exit 0");
        project
            .cookbooks
            .get_mut("bar")
            .unwrap()
            .recipes
            .get_mut("build")
            .unwrap()
            .run = String::from("exit 0");
        project
            .cookbooks
            .get_mut("bar")
            .unwrap()
            .recipes
            .get_mut("test")
            .unwrap()
            .run = String::from("exit 0");
        project
    }

    #[tokio::test]
    async fn run_all_recipes() {
        let project = Arc::new(create_test_project());
        let cache = build_cache(project.clone()).await;
        let res = super::bake(project.clone(), cache, None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn run_bar_recipes() {
        let mut project = create_test_project();
        project.config.verbose = true;
        let project = Arc::new(project);
        let cache = build_cache(project.clone()).await;
        let res = super::bake(project.clone(), cache, Some("bar:")).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn run_error_recipes() {
        let mut project = create_test_project(); // Project from builder has its dependency graph populated.

        // Modify bar:test to fail.
        project
            .cookbooks
            .get_mut("bar")
            .unwrap()
            .recipes
            .get_mut("test")
            .unwrap()
            .run = String::from("false; echo '''bar:test failed intentionally!'''");

        // Modify bar:build to depend on bar:test.
        project
            .cookbooks
            .get_mut("bar")
            .unwrap()
            .recipes
            .get_mut("build")
            .unwrap()
            .dependencies = Some(vec![String::from("bar:test")]);

        // After modifying dependencies, the project's recipe dependency graph needs to be repopulated.
        project
            .recipe_dependency_graph
            .populate_from_cookbooks(&project.cookbooks)
            .expect("Failed to repopulate graph for error test");

        let project_arc = Arc::new(project);
        let cache = build_cache(project_arc.clone()).await;
        let res = super::bake(project_arc.clone(), cache, Some("bar:")).await;

        // Assert that the bake operation failed as expected.
        assert!(res.is_err(), "Bake should fail when a recipe errors.");

        if let Err(e) = res {
            let error_message = e.to_string();
            assert!(
                error_message.contains("bar:test") && error_message.contains("failed"),
                "Error message should indicate that bar:test failed. Got: {error_message}"
            );
        }
    }
}
