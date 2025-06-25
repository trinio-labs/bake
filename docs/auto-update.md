# Auto-Update Feature

Bake includes an auto-update feature that allows the tool to automatically check for and install updates from GitHub releases.

## Configuration

You can configure the auto-update behavior in your `bake.yml` file:

```yaml
update:
  enabled: true                    # Enable/disable auto-update checks
  check_interval_days: 7          # How often to check for updates (default: 7)
  auto_update: false              # Automatically install updates (default: false)
  prerelease: false               # Include prereleases in update checks (default: false)
```

### Configuration Options

- **enabled**: When `true`, bake will check for updates during normal operation
- **check_interval_days**: Minimum days between update checks (to avoid excessive API calls)
- **auto_update**: When `true`, updates will be automatically downloaded and installed
- **prerelease**: When `true`, prerelease versions will be included in update checks

## How It Works

1. **Interval Checking**: Bake stores the last update check timestamp in the system cache directory
2. **Automatic Checks**: During normal operation, if auto-update is enabled, bake checks if enough time has passed since the last check
3. **Version Comparison**: The current version is compared against the latest release version
4. **User Notification**: If an update is available, users are notified with the version difference
5. **Download & Install**: If auto-update is enabled or the user runs `--self-update`, the new binary is downloaded and installed
6. **Restart Required**: After an update, users need to restart bake to use the new version

## CLI Commands

### Check for Updates

```bash
bake --check-updates
```

This command will check for available updates and display the result, regardless of the interval setting.

### Perform Self-Update

```bash
bake --self-update
```

This command will download and install the latest version of bake.

### Include Prereleases

```bash
bake --self-update --prerelease
```

This will update to the latest version, including prereleases.

### Show Update Information

```bash
bake --update-info
```

This displays information about the current installation.

## Storage

Update check timestamps are stored in the system cache directory:
- **macOS**: `~/Library/Caches/bake/last_update_check`
- **Linux**: `~/.cache/bake/last_update_check`
- **Windows**: `%LOCALAPPDATA%\bake\cache\last_update_check`

## Security

- Updates are downloaded from the official GitHub repository
- Binary integrity is verified using GitHub's release signatures
- The update process respects the user's existing permissions and installation location

## Troubleshooting

### Update Check Fails

If update checks fail, check:
- Internet connectivity
- GitHub API access
- Firewall settings

### Update Installation Fails

Common issues:
- Insufficient permissions to write to the binary location
- Antivirus software blocking the update
- Disk space issues

### Skip Update Checks

To skip update checks in specific environments:
- Set `CI=true` or `GITHUB_ACTIONS=true` environment variables
- Run from a development build (target/debug or target/release)
- Disable updates in the configuration file

### Reset Update Check Interval

To force an immediate update check, you can delete the timestamp file:
```bash
# macOS/Linux
rm ~/.cache/bake/last_update_check

# Windows
del "%LOCALAPPDATA%\bake\cache\last_update_check"
```

## Examples

### Basic Configuration

```yaml
# bake.yml
update:
  enabled: true
  check_interval_days: 7
  auto_update: false  # Manual updates only
```

### Automatic Updates

```yaml
# bake.yml
update:
  enabled: true
  check_interval_days: 1  # Check daily
  auto_update: true   # Automatically install updates
```

### Prerelease Testing

```yaml
# bake.yml
update:
  enabled: true
  check_interval_days: 3  # Check every 3 days
  prerelease: true    # Include prereleases
  auto_update: false  # Manual control for prereleases
``` 