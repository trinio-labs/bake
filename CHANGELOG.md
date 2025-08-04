# Bake

## Unreleased

## v0.9.0

### Added

- **Recipe Template System** - Complete DRY configuration system for reusable recipe definitions
  - Template definitions with typed parameters (string, number, boolean, array, object)
  - Parameter validation with defaults, required fields, regex patterns, and min/max constraints
  - Template discovery from `.bake/templates/` directories with automatic loading
  - Template instantiation with Handlebars parameter substitution using `{{ params.name }}` syntax
  - Template inheritance support with `extends` field for composition
  - Recipe field override system allowing templates and recipes to be combined flexibly

- **CLI Template Management** - New command-line tools for template discovery and validation
  - `--list-templates` argument displays all available templates with parameter details
  - `--validate-templates` argument performs comprehensive template validation
  - Colored output with status indicators and progress feedback
  - Detailed parameter information including types, requirements, defaults, and descriptions

- **Comprehensive JSON Schema System** - IDE validation and auto-completion support
  - Complete JSON schemas for `bake.yml`, `cookbook.yml`, and template files
  - IDE integration support (VS Code, JetBrains, Neovim) with ready-to-use configurations
  - Automated schema validation testing with comprehensive error reporting
  - GitHub-hosted schemas for universal access and CDN distribution
  - Schema catalog for public schema store publication with versioned URLs

- **Enhanced Documentation** - Comprehensive guides and examples
  - Complete recipe templates documentation with usage examples and best practices
  - JSON schema integration guides for popular IDEs and editors
  - Template parameter system documentation with validation rules
  - Real-world usage examples and migration guides

### Security

- **Comprehensive path traversal protection** - Robust security measures to prevent malicious tar archives
  - Absolute path rejection blocks paths like `/tmp/malicious.txt`
  - Path traversal protection prevents `../../../etc/passwd` attacks  
  - Canonical path validation ensures extracted files stay within project bounds
  - Cross-platform compatibility with macOS/Linux path resolution differences
  - Safe archive extraction with entry-by-entry validation replacing direct unpack operations

### Fixed

- **Recipe cache glob pattern handling** - Fixed errors with recipe cache when glob paths were relative and outside of cookbook directory
  - Improved relative path resolution for cache input patterns like `../../../libs/test_reader/**/*.go`
  - Enhanced canonical path handling for cross-platform compatibility
  - Better cookbook directory resolution with multi-level pattern support
  - Added comprehensive tests for complex relative path matching scenarios

- **Gitignore improvements** - Updated `.bake/cache` and `.bake/logs` patterns to use `**/.bake/` for better nested directory handling

### Technical

- Added `src/project/recipe_template.rs` with full template system implementation (500+ lines)
- Enhanced project loading with template discovery and resolution phases
- Extended recipe validation to ensure run commands from templates or direct definition
- Integrated template parameter validation with detailed error reporting
- Added comprehensive test coverage for template system functionality
- Created automated JSON schema validation suite for all configuration files
- Enhanced CLI argument parsing with template management commands
- Maintained full backward compatibility with existing projects

### Breaking Changes

- None - Template system is completely opt-in and backward compatible

## v0.8.1

### Fixed

- **Critical async I/O improvements** - Replaced all blocking filesystem operations with proper async variants in cache modules
- **Eliminated panic-prone code** - Replaced all `panic!` calls and dangerous `unwrap()` usage with proper error handling
- **Improved error handling** - Added graceful error handling for mutex locks and file operations
- **Enhanced code reliability** - Fixed potential runtime crashes and improved overall code stability
- **Test improvements** - Updated all tests to follow async patterns consistently

## v0.8.0

### Changed

- **BREAKING: Command format now requires ':' separator** - All recipe commands must now include a colon separator for
  consistency and clarity
  - `bake build` → `bake :build` or `bake cookbook:build`
  - `bake cookbook` → `bake cookbook:`
- Enhanced help text to clearly explain the new command format requirements
- Improved error messages with helpful guidance for correct usage

### Added

- **Colon-separated command parsing** - New structured command format with three patterns:
  - `bake cookbook:recipe` - Execute specific recipe from specific cookbook
  - `bake cookbook:` - Execute all recipes in a cookbook
  - `bake :recipe` - Execute all recipes with that name across all cookbooks
- **Regex pattern support** - Both cookbook and recipe parts now support full regex patterns:
  - `bake '^f.*:'` - Match cookbooks starting with 'f'
  - `bake ':^build'` - Match recipes starting with 'build'
  - `bake '^f.*:^build'` - Combine regex patterns for both parts
- **Comprehensive test coverage** - Added 22 new tests using `test_case` macro covering:
  - Pattern matching functionality
  - Error handling for invalid patterns
  - Regex pattern validation
  - Edge cases and no-match scenarios
  - Integration with execution planning

### Fixed

- **Code quality improvements** - Fixed all clippy warnings:
  - Removed redundant imports
  - Updated format strings to use inline arguments
  - Simplified complex type definitions
  - Improved code formatting and consistency

### Technical

- Added `filter_recipes_by_pattern()` method with comprehensive regex support
- Enhanced pattern validation with detailed error messages
- Improved test organization using parameterized tests
- Maintained backward compatibility for valid colon-separated patterns
- Added type aliases to reduce code complexity

## v0.7.0

### Changed

- **BREAKING: Configuration field naming** - Renamed `bake_version` to `config.minVersion` for better organization and clarity
- The minimum required bake version is now specified under the `config` section as `minVersion`
- Updated validation logic to use the new field location and naming convention

### Added

- **Enhanced update check behavior** - Manual update checks (`--check-updates`) now bypass the time interval and always
  check for updates
- Automatic background update checks still respect the configured check interval to avoid excessive API calls

### Fixed

- **Update check functionality** - Resolved issue where update checks were being skipped in development environments
- Added comprehensive debug logging to help diagnose update check issues

### Technical

- Refactored project configuration structure to move version requirements into the config section
- Updated serialization/deserialization logic for new field structure
- Enhanced test coverage for new configuration format and update functionality
- Maintained backward compatibility for projects without version specifications
- Added `force_check` parameter to update functions to distinguish manual vs automatic checks

## v0.6.1

This release fixes a critical issue where template variable substitution was converting all values to strings, causing
deserialization errors when configuration structs expected specific types like booleans.

### Fixed

- **Template type preservation** - Template variables now preserve their original YAML types (boolean, number, null)
  instead of always converting to strings
- **Boolean field support** - Configuration fields expecting boolean values now work correctly with template variables
- **Improved template processing** - Moved template processing logic to the appropriate module and simplified type
  conversion using serde_yaml's built-in parsing

### Technical

- Refactored `process_template_in_value` function to use serde_yaml's built-in type parsing
- Moved template processing from cookbook.rs to template.rs for better code organization
- Added comprehensive tests for type preservation in template processing
- Enhanced error handling for template variable resolution

## v0.6.0

This release adds comprehensive configuration variable support throughout bake projects, allowing variables to be used
in cache inputs/outputs, cookbook names, and all configuration files.

### Added

- **Comprehensive configuration variable support** - Variables can now be used throughout all configuration files
- **Cache variable support** - Cache inputs and outputs can now use templated variables
- **Cookbook name templating** - Cookbook names can be dynamically generated using variables
- **Project-wide variable context** - Enhanced variable system with improved scoping and inheritance
- Version management in project configuration files
- `--update-version` flag to update project configuration to current bake version
- `--force-version-override` flag to run even with newer config versions
- Enhanced template system with improved variable context handling
- Better error handling and validation for project configurations

### Changed

- **Template system overhaul** - Complete refactor of the template system for better variable support
- Improved cache handling and reliability with variable support
- Enhanced cookbook processing and validation with templated names
- Updated test configurations and examples to showcase new variable capabilities
- **Breaking change**: Enhanced variable context and scoping rules

### Technical

- Refactored template system for comprehensive variable support
- Improved project configuration parsing and validation
- Enhanced error messages and user feedback for variable resolution
- Better variable inheritance and scoping mechanisms

## v0.5.0

This release adds self-update functionality to bake, allowing users to automatically check for and install updates.

### Added

- Self-update functionality with automatic update checks
- `bake self-update` command to manually update to the latest version
- Configurable update settings (enabled/disabled, check interval, auto-update, prerelease support)
- Update notifications when new versions are available
- Support for Homebrew installation via `cargo-dist`

### Changed

- Updated to use `cargo-dist` for release management and distribution
- Improved error handling and user feedback for update operations
- Enhanced CI/CD pipeline with automated releases and Homebrew formula updates

### Technical

- Added `self_update` dependency for handling binary updates
- Implemented update checking with configurable intervals
- Added cache-based update check throttling to avoid excessive API calls
- Skip update checks in CI environments and development builds

### Fixes

- Fixed bug where a cache input dependency couldn't be in parent folders or using relative paths

## v0.4.9

Minor bug corrections

## v0.4.8

This release changes the behavior of recipe execution. Recipe commands are now
run in a shell that sets `set -e` before any other command. This ensures that intermediate commands
fail fast and trigger a recipe failure.

### Changes

- Run commands with `set -e`

## v0.4.7

This release tries to fix the tar error during a cache retrieval. Changes compression algorithm to zstd.

### Changes

- Compression algorithm changed from gzip to zstd due to faster decompression

## v0.4.6

This release changes how bake chooses to cache a recipe. It now only caches if the recipe has
a cache property in its definition.

### Added

- Recipe is only cached if it has a cache property

## v0.4.5

This release adds a few more debug logs to GCS cache and adds an elapsed running time to the verbose output.

## v0.4.4

This release adds the external-account feature to google cloud storage.

## v0.4.3

Added more information about recipes that ran with errors.

### Added

- Recipes don't run anymore if any dependencies failed running
- Bake now shows the recipes that failed after a bake

## v0.4.2

This release fixes a bug with the cache input globs.

### Fixes

Fixes a bug with cache input globs that wouldn't correctly match files when they were in a subdirectory.

## v0.4.1

Updates dependencies, hopefully to fix remote caching to GCS and Workload Identity Federation.

### Fixes

- Fixes workload identity federation by updating google-cloud-auth

## v0.4.0

This release changes the way bake executes recipes that use environment variables. Starting from now, bake will only execute
in a clean environment if the clean_environment property is set to true in the project config file.

### Added

- Clean environment flag in project configuration file

## v0.3.1

Bug fixes

### Changes

- Fixes [#1](https://github.com/trinio-labs/bake/issues/1)

## v0.3.0

This release adds support for passing variables via command line and adds the concept of native project variables.

### Added

- Command line option to override project variables
- Usage of `{{ project.root }}` and `{{ cookbook.root }}` in config files

### Breaking changes

- Output and inputs are moved to the cache property in recipes

## v0.2.0

This release adds support for variables in bake projects as well as using handlebar to define variables and run configurations.

### Added

- Templated variables and run configurations
- Project, cookbook and recipe variables
- Further documentation for config files
- Execution environments with clean environment variables

## v0.1.2

This release is the first stable version correcting a few bugs and adding remote caching on S3 and GCS.

### Added

- GCS Support
- S3 Support

## v0.1.1

### Changed

Updated name of project to bake-cli so we can upload it to crates.io. The name bake is being used by
a similar but abandoned project.

## v0.1.0

Initial release.
