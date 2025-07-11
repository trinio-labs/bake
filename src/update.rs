use anyhow::{anyhow, Result};
use console::style;
use self_update::{backends::github::Update, cargo_crate_version};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_REPO_OWNER: &str = "trinio-labs";
const GITHUB_REPO_NAME: &str = "bake";
const BINARY_NAME: &str = "bake";

/// Configuration for auto-update functionality
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub enabled: bool,
    pub check_interval_days: u64,
    pub auto_update: bool,
    pub prerelease: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_days: 7,
            auto_update: false,
            prerelease: false,
        }
    }
}

/// Get the path to the update check timestamp file
fn get_update_check_file(cache_dir: Option<&PathBuf>) -> Result<PathBuf> {
    let cache_dir = match cache_dir {
        Some(dir) => dir.clone(),
        None => dirs::cache_dir()
            .ok_or_else(|| anyhow!("Could not determine cache directory"))?
            .join("bake"),
    };
    // Create cache directory if it doesn't exist
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir.join("last_update_check"))
}

/// Check if enough time has passed since the last update check
fn should_check_for_updates(
    config: &UpdateConfig,
    cache_dir: Option<&PathBuf>,
    force_check: bool,
) -> Result<bool> {
    // If force_check is true (manual check), always allow the check
    if force_check {
        return Ok(true);
    }

    let check_file = get_update_check_file(cache_dir)?;
    if !check_file.exists() {
        return Ok(true);
    }
    let last_check_str = fs::read_to_string(&check_file)?;
    let last_check: u64 = last_check_str.trim().parse()?;
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let days_since_last_check = (current_time - last_check) / (24 * 60 * 60);
    Ok(days_since_last_check >= config.check_interval_days)
}

/// Update the last check timestamp
fn update_last_check_timestamp(cache_dir: Option<&PathBuf>) -> Result<()> {
    let check_file = get_update_check_file(cache_dir)?;
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    fs::write(check_file, current_time.to_string())?;
    Ok(())
}

/// Check if an update is available and optionally perform the update
pub async fn check_for_updates(config: &UpdateConfig, force_check: bool) -> Result<Option<String>> {
    if !config.enabled {
        return Ok(None);
    }
    if should_skip_update_check() {
        return Ok(None);
    }
    if !should_check_for_updates(config, None, force_check)? {
        return Ok(None);
    }
    let current_version = cargo_crate_version!();
    match Update::configure()
        .repo_owner(GITHUB_REPO_OWNER)
        .repo_name(GITHUB_REPO_NAME)
        .bin_name(BINARY_NAME)
        .bin_path_in_archive("bake-cli-{{ target }}/{{ bin }}")
        .current_version(current_version)
        .show_download_progress(true)
        .build()
    {
        Ok(updater) => match updater.get_latest_release() {
            Ok(release) => {
                let latest_version = release.version;
                if !config.prerelease && is_prerelease(&latest_version) {
                    let _ = update_last_check_timestamp(None);
                    return Ok(None);
                }
                if latest_version != current_version {
                    println!(
                        "{} {} → {}",
                        style("Update available:").yellow(),
                        style(current_version).dim(),
                        style(&latest_version).green()
                    );
                    if config.auto_update {
                        println!("{}", style("Auto-updating...").cyan());
                        match updater.update() {
                            Ok(_) => {
                                println!("{}", style("✓ Update completed successfully!").green());
                                println!(
                                    "{}",
                                    style("Please restart bake to use the new version.").dim()
                                );
                                let _ = update_last_check_timestamp(None);
                                return Ok(Some(latest_version));
                            }
                            Err(e) => {
                                eprintln!("{}: {}", style("✗ Update failed").red(), e);
                                return Err(anyhow!("Failed to update: {}", e));
                            }
                        }
                    } else {
                        println!(
                            "{} {}",
                            style("Run").dim(),
                            style("bake --self-update").cyan()
                        );
                        let _ = update_last_check_timestamp(None);
                        return Ok(Some(latest_version));
                    }
                } else {
                    let _ = update_last_check_timestamp(None);
                }
            }
            Err(e) => {
                log::warn!("Failed to check for updates: {e}");
            }
        },
        Err(e) => {
            log::warn!("Failed to configure updater: {e}");
        }
    }
    Ok(None)
}

/// Perform a self-update
pub fn perform_self_update(prerelease: bool) -> Result<()> {
    let current_version = cargo_crate_version!();

    let updater = Update::configure()
        .repo_owner(GITHUB_REPO_OWNER)
        .repo_name(GITHUB_REPO_NAME)
        .bin_name(BINARY_NAME)
        .bin_path_in_archive("bake-cli-{{ target }}/{{ bin }}")
        .current_version(current_version)
        .show_download_progress(true)
        .build()?;

    let latest_release = updater.get_latest_release()?;
    let latest_version = latest_release.version;

    // Skip prereleases unless explicitly requested
    if !prerelease && is_prerelease(&latest_version) {
        println!(
            "{}",
            style("Latest version is a prerelease. Use --prerelease to update to it.").yellow()
        );
        return Ok(());
    }

    if latest_version == current_version {
        println!("{}", style("✓ Already up to date!").green());
        return Ok(());
    }

    println!(
        "{} {} → {}",
        style("Updating bake:").cyan(),
        style(current_version).dim(),
        style(&latest_version).green()
    );

    match updater.update() {
        Ok(_) => {
            println!("{}", style("✓ Update completed successfully!").green());
            println!(
                "{}",
                style("Please restart bake to use the new version.").dim()
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("{}: {}", style("✗ Update failed").red(), e);
            Err(anyhow!("Failed to update: {}", e))
        }
    }
}

/// Check if a version string represents a prerelease
fn is_prerelease(version: &str) -> bool {
    version.contains('-')
        || version.contains("alpha")
        || version.contains("beta")
        || version.contains("rc")
}

/// Check if we should skip the update check
fn should_skip_update_check() -> bool {
    // Skip in CI environments
    if env::var("CI").is_ok() || env::var("GITHUB_ACTIONS").is_ok() {
        return true;
    }

    // Skip in development (when running from cargo)
    if env::var("CARGO").is_ok() {
        return true;
    }

    // Skip if running from a development build
    if let Ok(current_exe) = env::current_exe() {
        if current_exe.to_string_lossy().contains("target/debug")
            || current_exe.to_string_lossy().contains("target/release")
        {
            return true;
        }
    }

    false
}

/// Get update status information
pub fn get_update_info() -> Result<String> {
    let current_version = cargo_crate_version!();
    let current_exe = env::current_exe()?;

    Ok(format!(
        "Version: {}\nBinary: {}",
        current_version,
        current_exe.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_days, 7);
        assert!(!config.auto_update);
        assert!(!config.prerelease);
    }

    #[test]
    fn test_should_skip_update_check_in_ci() {
        env::set_var("CI", "true");
        assert!(should_skip_update_check());
        env::remove_var("CI");
    }

    #[test]
    fn test_should_skip_update_check_in_cargo() {
        env::set_var("CARGO", "true");
        assert!(should_skip_update_check());
        env::remove_var("CARGO");
    }

    #[test]
    fn test_get_update_info() {
        let info = get_update_info().unwrap();
        assert!(info.contains("Version:"));
        assert!(info.contains("Binary:"));
    }

    #[test]
    fn test_should_check_for_updates_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let bake_cache = temp_dir.path().join("bake");
        let config = UpdateConfig {
            enabled: true,
            check_interval_days: 7,
            auto_update: false,
            prerelease: false,
        };
        // Should check if no file exists
        assert!(should_check_for_updates(&config, Some(&bake_cache), false).unwrap());
    }

    #[test]
    fn test_should_check_for_updates_with_recent_file() {
        let temp_dir = TempDir::new().unwrap();
        let bake_cache = temp_dir.path().join("bake");
        fs::create_dir_all(&bake_cache).unwrap();
        // Write a recent timestamp (1 day ago)
        let recent_timestamp = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 24 * 60 * 60)
            .to_string();
        fs::write(bake_cache.join("last_update_check"), recent_timestamp).unwrap();
        let config = UpdateConfig {
            enabled: true,
            check_interval_days: 7,
            auto_update: false,
            prerelease: false,
        };
        // Should not check if file is recent
        assert!(!should_check_for_updates(&config, Some(&bake_cache), false).unwrap());
    }

    #[test]
    fn test_should_check_for_updates_with_old_file() {
        let temp_dir = TempDir::new().unwrap();
        let bake_cache = temp_dir.path().join("bake");
        fs::create_dir_all(&bake_cache).unwrap();
        // Write an old timestamp (10 days ago)
        let old_timestamp = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 10 * 24 * 60 * 60)
            .to_string();
        fs::write(bake_cache.join("last_update_check"), old_timestamp).unwrap();
        let config = UpdateConfig {
            enabled: true,
            check_interval_days: 7,
            auto_update: false,
            prerelease: false,
        };
        // Should check if file is old
        assert!(should_check_for_updates(&config, Some(&bake_cache), false).unwrap());
    }

    #[test]
    fn test_should_check_for_updates_force_check() {
        let temp_dir = TempDir::new().unwrap();
        let bake_cache = temp_dir.path().join("bake");
        fs::create_dir_all(&bake_cache).unwrap();
        // Write a recent timestamp (1 day ago)
        let recent_timestamp = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 24 * 60 * 60)
            .to_string();
        fs::write(bake_cache.join("last_update_check"), recent_timestamp).unwrap();
        let config = UpdateConfig {
            enabled: true,
            check_interval_days: 7,
            auto_update: false,
            prerelease: false,
        };
        // Should check even with recent file when force_check is true
        assert!(should_check_for_updates(&config, Some(&bake_cache), true).unwrap());
        // Should not check with recent file when force_check is false
        assert!(!should_check_for_updates(&config, Some(&bake_cache), false).unwrap());
    }

    #[test]
    fn test_is_prerelease() {
        assert!(is_prerelease("1.0.0-alpha"));
        assert!(is_prerelease("1.0.0-beta"));
        assert!(is_prerelease("1.0.0-rc.1"));
        assert!(is_prerelease("1.0.0-alpha.1"));
        assert!(!is_prerelease("1.0.0"));
        assert!(!is_prerelease("2.1.3"));
    }
}
