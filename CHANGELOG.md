# Bake

## Unreleased

## v0.6.1

This release fixes a critical issue where template variable substitution was converting all values to strings, causing deserialization errors when configuration structs expected specific types like booleans.

### Fixed

* **Template type preservation** - Template variables now preserve their original YAML types (boolean, number, null) instead of always converting to strings
* **Boolean field support** - Configuration fields expecting boolean values now work correctly with template variables
* **Improved template processing** - Moved template processing logic to the appropriate module and simplified type conversion using serde_yaml's built-in parsing

### Technical

* Refactored `process_template_in_value` function to use serde_yaml's built-in type parsing
* Moved template processing from cookbook.rs to template.rs for better code organization
* Added comprehensive tests for type preservation in template processing
* Enhanced error handling for template variable resolution

## v0.6.0

This release adds comprehensive configuration variable support throughout bake projects, allowing variables to be used in cache inputs/outputs, cookbook names, and all configuration files.

### Added

* **Comprehensive configuration variable support** - Variables can now be used throughout all configuration files
* **Cache variable support** - Cache inputs and outputs can now use templated variables
* **Cookbook name templating** - Cookbook names can be dynamically generated using variables
* **Project-wide variable context** - Enhanced variable system with improved scoping and inheritance
* Version management in project configuration files
* `--update-version` flag to update project configuration to current bake version
* `--force-version-override` flag to run even with newer config versions
* Enhanced template system with improved variable context handling
* Better error handling and validation for project configurations

### Changed

* **Template system overhaul** - Complete refactor of the template system for better variable support
* Improved cache handling and reliability with variable support
* Enhanced cookbook processing and validation with templated names
* Updated test configurations and examples to showcase new variable capabilities
* **Breaking change**: Enhanced variable context and scoping rules

### Technical

* Refactored template system for comprehensive variable support
* Improved project configuration parsing and validation
* Enhanced error messages and user feedback for variable resolution
* Better variable inheritance and scoping mechanisms

## v0.5.0

This release adds self-update functionality to bake, allowing users to automatically check for and install updates.

### Added

* Self-update functionality with automatic update checks
* `bake self-update` command to manually update to the latest version
* Configurable update settings (enabled/disabled, check interval, auto-update, prerelease support)
* Update notifications when new versions are available
* Support for Homebrew installation via `cargo-dist`

### Changed

* Updated to use `cargo-dist` for release management and distribution
* Improved error handling and user feedback for update operations
* Enhanced CI/CD pipeline with automated releases and Homebrew formula updates

### Technical

* Added `self_update` dependency for handling binary updates
* Implemented update checking with configurable intervals
* Added cache-based update check throttling to avoid excessive API calls
* Skip update checks in CI environments and development builds

### Fixes
* Fixed bug where a cache input dependency couldn't be in parent folders or using relative paths


## v0.4.9

Minor bug corrections


## v0.4.8

This release changes the behavior of recipe execution. Recipe commands are now
run in a shell that sets `set -e` before any other command. This ensures that intermediate commands
fail fast and trigger a recipe failure.

### Changes

* Run commands with `set -e`

## v0.4.7

This release tries to fix the tar error during a cache retrieval. Changes compression algorithm to zstd.

### Changes

* Compression algorithm changed from gzip to zstd due to faster decompression

## v0.4.6

This release changes how bake chooses to cache a recipe. It now only caches if the recipe has
a cache property in its definition.

### Added

* Recipe is only cached if it has a cache property

## v0.4.5

This release adds a few more debug logs to GCS cache and adds an elapsed running time to the verbose output.

## v0.4.4

This release adds the external-account feature to google cloud storage.

## v0.4.3

Added more information about recipes that ran with errors.

### Added

* Recipes don't run anymore if any dependencies failed running
* Bake now shows the recipes that failed after a bake

## v0.4.2

This release fixes a bug with the cache input globs.

### Fixes

Fixes a bug with cache input globs that wouldn't correctly match files when they were in a subdirectory.

## v0.4.1

Updates dependencies, hopefully to fix remote caching to GCS and Workload Identity Federation.

### Fixes

* Fixes workload identity federation by updating google-cloud-auth

## v0.4.0

This release changes the way bake executes recipes that use environment variables. Starting from now, bake will only execute
in a clean environment if the clean_environment property is set to true in the project config file.

### Added

* Clean environment flag in project configuration file

## v0.3.1

Bug fixes

### Changes

* Fixes [#1](https://github.com/trinio-labs/bake/issues/1)

## v0.3.0

This release adds support for passing variables via command line and adds the concept of native project variables.

### Added

* Command line option to override project variables
* Usage of `{{ project.root }}` and `{{ cookbook.root }}` in config files

### Breaking changes

* Output and inputs are moved to the cache property in recipes

## v0.2.0

This release adds support for variables in bake projects as well as using handlebar to define variables and run configurations.

### Added

* Templated variables and run configurations
* Project, cookbook and recipe variables
* Further documentation for config files
* Execution environments with clean environment variables

## v0.1.2

This release is the first stable version correcting a few bugs and adding remote caching on S3 and GCS.

### Added

* GCS Support
* S3 Support

## v0.1.1

### Changed

Updated name of project to bake-cli so we can upload it to crates.io. The name bake is being used by
a similar but abandoned project.

## v0.1.0

Initial release.
