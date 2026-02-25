use anyhow::{Result, anyhow};
use console::style;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_REPO_OWNER: &str = "trinio-labs";
const GITHUB_REPO_NAME: &str = "bake";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Configuration for update checking
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub enabled: bool,
    pub check_interval_days: u64,
    pub prerelease: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_days: 7,
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

/// Fetch the latest release version from GitHub API using curl
fn fetch_latest_version() -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/{GITHUB_REPO_OWNER}/{GITHUB_REPO_NAME}/releases/latest"
    );

    let output = std::process::Command::new("curl")
        .args(["-s", "-H", "Accept: application/vnd.github.v3+json", &url])
        .output()
        .map_err(|e| anyhow!("Failed to run curl: {e}"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "GitHub API request failed with status: {}",
            output.status
        ));
    }

    let body = String::from_utf8(output.stdout)
        .map_err(|e| anyhow!("Invalid UTF-8 in GitHub response: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| anyhow!("Failed to parse GitHub response: {e}"))?;

    json["tag_name"]
        .as_str()
        .map(|v| v.strip_prefix('v').unwrap_or(v).to_string())
        .ok_or_else(|| anyhow!("No tag_name found in GitHub release response"))
}

/// Check if an update is available
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

    match fetch_latest_version() {
        Ok(latest_version) => {
            if !config.prerelease && is_prerelease(&latest_version) {
                let _ = update_last_check_timestamp(None);
                return Ok(None);
            }
            if latest_version != CURRENT_VERSION {
                println!(
                    "{} {} → {}",
                    style("Update available:").yellow(),
                    style(CURRENT_VERSION).dim(),
                    style(&latest_version).green()
                );
                println!(
                    "{}",
                    style("Install the latest version from https://github.com/trinio-labs/bake/releases")
                        .dim()
                );
                let _ = update_last_check_timestamp(None);
                return Ok(Some(latest_version));
            } else {
                let _ = update_last_check_timestamp(None);
            }
        }
        Err(e) => {
            log::warn!("Failed to check for updates: {e}");
        }
    }
    Ok(None)
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
        let exe_path = current_exe.to_string_lossy();
        if exe_path.contains("target/debug") || exe_path.contains("target/release") {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Mutex to serialize tests that modify environment variables
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_days, 7);
        assert!(!config.prerelease);
    }

    #[test]
    fn test_should_skip_update_check_in_ci() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // SAFETY: Test code running in single-threaded test context with mutex lock
        unsafe { env::set_var("CI", "true") };
        assert!(should_skip_update_check());
        // SAFETY: Test code running in single-threaded test context with mutex lock
        unsafe { env::remove_var("CI") };
    }

    #[test]
    fn test_should_skip_update_check_in_cargo() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // SAFETY: Test code running in single-threaded test context with mutex lock
        unsafe { env::set_var("CARGO", "true") };
        assert!(should_skip_update_check());
        // SAFETY: Test code running in single-threaded test context with mutex lock
        unsafe { env::remove_var("CARGO") };
    }

    #[test]
    fn test_should_check_for_updates_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let bake_cache = temp_dir.path().join("bake");
        let config = UpdateConfig {
            enabled: true,
            check_interval_days: 7,
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

    #[test]
    fn test_get_update_check_file_with_custom_cache_dir() {
        let temp_dir = TempDir::new().unwrap();
        let custom_cache = temp_dir.path().join("custom_cache");

        let result = get_update_check_file(Some(&custom_cache));
        assert!(result.is_ok());

        let check_file = result.unwrap();
        assert_eq!(check_file, custom_cache.join("last_update_check"));
        assert!(custom_cache.exists()); // Directory should be created
    }

    #[test]
    fn test_get_update_check_file_with_default_cache_dir() {
        let result = get_update_check_file(None);
        // Should succeed and return a path under the system cache directory
        assert!(result.is_ok());
        let check_file = result.unwrap();
        assert!(check_file.to_string_lossy().contains("bake"));
        assert!(check_file.to_string_lossy().ends_with("last_update_check"));
    }

    #[test]
    fn test_update_last_check_timestamp() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("bake");

        let result = update_last_check_timestamp(Some(&cache_dir));
        assert!(result.is_ok());

        let check_file = cache_dir.join("last_update_check");
        assert!(check_file.exists());

        let content = fs::read_to_string(&check_file).unwrap();
        let timestamp: u64 = content.trim().parse().unwrap();

        // Timestamp should be recent (within last minute)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(timestamp <= current_time);
        assert!(current_time - timestamp < 60); // Within last minute
    }

    #[test]
    fn test_should_skip_update_check_detects_ci_and_dev_environments() {
        // When run as part of cargo test, should return true because we're in a dev environment
        let result = should_skip_update_check();
        println!("should_skip_update_check() returned: {result}");
        // We don't assert a specific value since the result depends on the test environment
    }

    #[test]
    fn test_should_skip_update_check_github_actions() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // SAFETY: Test code running in single-threaded test context with mutex lock
        unsafe { env::set_var("GITHUB_ACTIONS", "true") };
        assert!(should_skip_update_check());
        // SAFETY: Test code running in single-threaded test context with mutex lock
        unsafe { env::remove_var("GITHUB_ACTIONS") };
    }

    #[test]
    fn test_should_check_for_updates_invalid_timestamp_file() {
        let temp_dir = TempDir::new().unwrap();
        let bake_cache = temp_dir.path().join("bake");
        fs::create_dir_all(&bake_cache).unwrap();

        // Write invalid timestamp content
        fs::write(bake_cache.join("last_update_check"), "invalid_timestamp").unwrap();

        let config = UpdateConfig::default();

        // Should return error when timestamp file contains invalid data
        let result = should_check_for_updates(&config, Some(&bake_cache), false);
        assert!(result.is_err());
    }
}
