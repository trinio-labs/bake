use std::{
    collections::HashMap,
    fs::File,
    io::prelude::*,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use console::{style, Color};
use indicatif::{MultiProgress, ProgressBar};
use log::debug;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{ChildStderr, ChildStdout},
    sync::mpsc,
    task::JoinSet,
    time,
};

use crate::project::{BakeProject, Recipe, RecipeSearch};

type RecipeQueue = Arc<Mutex<Vec<Recipe>>>;
type StatusMap = Arc<Mutex<HashMap<String, RunStatus>>>;

enum Status {
    Done,
    Error,
    Idle,
    Running,
}
struct RunStatus {
    status: Status,
    output: String,
}

/// Bakes a project by running all recipes and their dependencies
///
/// # Arguments
/// * `project` - The project to bake
/// * `filter` - Optional recipe pattern to filter such as `foo:`
///
pub async fn bake(project: BakeProject, filter: Option<&str>) -> Result<(), String> {
    let filtered_recipes: Vec<&Recipe> = if let Some(filter) = filter {
        project.recipes(RecipeSearch::ByPattern(filter))
    } else {
        project.recipes(RecipeSearch::All)
    };

    let all_status = filtered_recipes
        .iter()
        .map(|recipe| {
            let status = RunStatus {
                status: Status::Idle,
                output: String::new(),
            };
            (recipe.full_name().clone(), status)
        })
        .collect();
    let status_map: StatusMap = Arc::new(Mutex::new(all_status));
    let recipe_queue =
        RecipeQueue::new(Mutex::new(filtered_recipes.into_iter().cloned().collect()));
    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();
    let mut join_set = JoinSet::new();
    let arc_project = Arc::new(project);

    let multi_progress = Arc::new(MultiProgress::new());

    (0..arc_project.config.max_parallel).for_each(|_| {
        let shutdown_tx = shutdown_tx.clone();
        let arc_project = arc_project.clone();
        let recipe_queue = recipe_queue.clone();
        let status_map = status_map.clone();
        let multi_progress = multi_progress.clone();

        join_set.spawn(runner(
            arc_project,
            recipe_queue,
            status_map,
            shutdown_tx,
            multi_progress,
        ));
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            join_set.abort_all();
        },
        _ = shutdown_rx.recv() => {
            join_set.abort_all();
        },
        _ = async {
            // Wait for joinset to finish running
            while (join_set.join_next().await).is_some() {}
        } => {}
    }

    if status_map
        .lock()
        .unwrap()
        .iter()
        .any(|(_, status)| matches!(status.status, Status::Error))
    {
        return Err("Some recipes failed to run".to_string());
    }

    Ok(())
}

/// Runners are spawned in parallel to run recipes that were added to the queue
///
/// runner also handles printing the progress bar to the console if needed
///
/// # Arguments
/// * `project` - The project to bake
/// * `recipe_queue` - The shared queue of recipes
/// * `status_map` - The shared status map
/// * `shutdown_tx` - The channel to send shutdown signals
/// * `multi_progress` - The multi progress bar
///
async fn runner(
    project: Arc<BakeProject>,
    recipe_queue: RecipeQueue,
    status_map: StatusMap,
    shutdown_tx: mpsc::UnboundedSender<()>,
    multi_progress: Arc<MultiProgress>,
) -> Result<(), String> {
    loop {
        let mut next_recipe: Option<Recipe> = None;
        if let Ok(mut queue) = recipe_queue.lock() {
            // If there are no more recipes to process, quit runner loop
            if queue.is_empty() {
                break;
            }
            let next_recipe_pos = queue.iter().position(|recipe| {
                if let Some(dependencies) = recipe.dependencies.as_ref() {
                    let pending = dependencies.iter().any(|dep_name| {
                        // If the dependency isn't in the status map, allow it to "run" anyway as we will
                        // filter it later
                        if let Some(rec_status) = status_map.lock().unwrap().get(dep_name) {
                            matches!(rec_status.status, Status::Running | Status::Idle)
                        } else {
                            false
                        }
                    });
                    !pending
                } else {
                    true
                }
            });
            if let Some(pos) = next_recipe_pos {
                if let Some(dependencies) = queue[pos].dependencies.clone().as_ref() {
                    // If any of the dependencies aren't in the status map add it to the queue and
                    // status map
                    let mut untracked_dep = false;
                    dependencies.iter().for_each(|dep_name| {
                        if status_map.lock().unwrap().get(dep_name).is_none() {
                            debug!(
                                "Dependency {} not in status map, adding it to queue",
                                dep_name
                            );
                            let dep_recipe = project.get_recipe_by_name(dep_name).unwrap();
                            queue.push(dep_recipe.clone());
                            status_map.lock().unwrap().insert(
                                dep_name.to_string(),
                                RunStatus {
                                    status: Status::Idle,
                                    output: String::new(),
                                },
                            );
                            untracked_dep = true;
                        }
                    });
                    if untracked_dep {
                        continue;
                    }
                }
                next_recipe = Some(queue.remove(pos));
            }
        }

        if let Some(next_recipe) = next_recipe {
            let mut progress_bar: Option<ProgressBar> = None;
            if !project.config.verbose {
                progress_bar = Some(
                    multi_progress.add(
                        ProgressBar::new_spinner()
                            .with_message(format!("Baking recipe {}...", next_recipe.full_name())),
                    ),
                );
            }
            // Run async tasks until one of them finishes
            tokio::select! {
                // Update progress bar if present
                _ = async {
                    loop {
                        if let Some(progress_bar) = progress_bar.as_ref() {
                            progress_bar.tick();
                        }
                        time::sleep(time::Duration::from_millis(100)).await;
                    }
                } => {},
                // Update status and run recipe asynchronously, awaiting for the result
                _ = async {
                    {
                        let mut status_mutex = status_map.lock().unwrap();
                        let status = status_mutex.get_mut(&next_recipe.full_name()).unwrap();
                        status.status = Status::Running;
                    }

                    let result = run_recipe(&next_recipe, &project.root_path, project.config.verbose).await;

                    let mut status_mutex = status_map.lock().unwrap();
                    let status = status_mutex.get_mut(&next_recipe.full_name()).unwrap();

                    match result {
                        Ok(_) => {
                            status.status = Status::Done;
                            if let Some(progress_bar) = progress_bar.as_ref() {
                            progress_bar.finish_with_message(format!(
                                "Baking recipe {}... {}",
                                next_recipe.full_name(),
                                console::style("✓").green()
                            ));
                            }
                        }
                        Err(err) => {
                            if let Some(progress_bar) = progress_bar.as_ref() {
                            progress_bar.finish_with_message(format!(
                                "Baking recipe {}... {}",
                                next_recipe.full_name(),
                                console::style("✗").red()
                            ));
                            }
                            if project.config.fast_fail {
                                shutdown_tx.send(()).unwrap();
                            }
                            status.status = Status::Error;
                            status.output = err;
                        }
                    }
                } => {}
            }
        } else {
            time::sleep(time::Duration::from_millis(100)).await;
        }
    }

    Ok(())
}

/// Runs a single recipe as a system process and handles the output
///
/// # Arguments
/// * `recipe` - The recipe to run
/// * `project_root` - The root path of the project
/// * `verbose` - Whether to print verbose output
///
pub async fn run_recipe(recipe: &Recipe, project_root: &Path, verbose: bool) -> Result<(), String> {
    // TODO: Implement cache strategy
    let result = tokio::process::Command::new("sh")
        .current_dir(recipe.config_path.parent().unwrap())
        .arg("-c")
        .arg(recipe.run.clone())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match result {
        Ok(mut child) => {
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let process_handle = tokio::spawn(process_output(
                stdout,
                stderr,
                recipe.full_name(),
                project_root.to_path_buf(),
                verbose,
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
                return Err(format!("Could wait for process output thread: {}", err));
            }
        }
        Err(err) => {
            return Err(format!("Could not spawn process: {}", err));
        }
    }

    Ok(())
}

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

/// Processes the output of a process saving it to a file and printing to console if in verbose
/// mode
///
/// # Arguments
/// * `stdout` - The stdout of the process
/// * `stderr` - The stderr of the process
/// * `recipe_name` - The name of the recipe
/// * `project_root` - The root path of the project
/// * `verbose` - Whether to print verbose output
///
async fn process_output(
    stdout: ChildStdout,
    stderr: ChildStderr,
    recipe_name: String,
    project_root: PathBuf,
    verbose: bool,
) -> Result<(), String> {
    let mut join_set = JoinSet::new();
    let color = name_to_term_color(&recipe_name);
    let output_str = Arc::new(Mutex::new(String::new()));

    async fn collect_output<T: AsyncRead + Unpin>(
        output: T,
        recipe_name: String,
        color: Color,
        output_string: Arc<Mutex<String>>,
        verbose: bool,
    ) {
        let mut reader = BufReader::new(output).lines();
        while let Some(line) = reader.next_line().await.unwrap() {
            let formatted_line = format!("[{}]: {}", style(&recipe_name).fg(color), line);
            if verbose {
                println!("{formatted_line}");
            }
            output_string.lock().unwrap().push_str(&(line + "\n"));
        }
    }

    join_set.spawn(collect_output(
        stdout,
        recipe_name.clone(),
        color,
        output_str.clone(),
        verbose,
    ));

    join_set.spawn(collect_output(
        stderr,
        recipe_name.clone(),
        color,
        output_str.clone(),
        verbose,
    ));

    while (join_set.join_next().await).is_some() {}

    if let Err(err) = std::fs::create_dir_all(project_root.join(".bake")) {
        return Err(format!("Could not create directory .bake: {}", err));
    };
    let log_file_path = project_root.join(format!(".bake/{}.log", recipe_name.replace(':', ".")));

    match File::create(log_file_path.clone()) {
        Ok(mut file) => {
            if let Err(err) = file.write_all(output_str.lock().unwrap().as_bytes()) {
                return Err(format!(
                    "Could not write log file {}: {}",
                    log_file_path.display(),
                    err
                ));
            };
        }
        Err(err) => {
            return Err(format!(
                "Could not create log file {}: {}",
                log_file_path.display(),
                err
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::project::BakeProject;

    #[tokio::test]
    async fn run_all_recipes() {
        let project = BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap();
        let res = super::bake(project, None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn run_bar_recipes() {
        let mut project = BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap();
        project.config.verbose = false;
        let res = super::bake(project, Some("bar:")).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn run_error_recipes() {
        let mut project = BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap();
        project
            .cookbooks
            .get_mut("bar")
            .unwrap()
            .recipes
            .get_mut("test")
            .unwrap()
            .run = String::from("ex12123123");
        let res = super::bake(project, Some("bar:")).await;
        assert!(res.is_err());
    }
}
