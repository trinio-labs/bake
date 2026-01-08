# Bake

## v2.0.2 - 2026-01-08

### Fixed

- **Remote manifest storage for CI cache sharing** - Action cache manifests are now stored in remote blob stores (S3/GCS)
  - Previously, manifests were stored locally only, causing cache misses on fresh CI machines
  - Manifests are now uploaded alongside blobs for full cache portability across machines
  - Respects configured cache order (LocalFirst vs RemoteFirst)
  - Automatic promotion: manifests fetched from remote are cached locally

## v2.0.1 - 2026-01-06

### Fixed

- **S3 cache compatibility with ACL-disabled buckets** - Fixed S3 cache save failing on buckets with ACLs disabled
  - Removed `ObjectCannedAcl::BucketOwnerFullControl` from put operations
  - S3 buckets using `ObjectOwnership: BucketOwnerEnforced` (ACLs disabled) now work correctly
  - This is the default setting for new S3 buckets created after April 2023

- **S3 "not found" detection** - Improved detection of missing blobs in S3 cache
  - Now uses AWS SDK's `is_not_found()` method instead of string matching
  - Eliminates spurious "S3 head_object error... (treating as miss)" warnings
  - More reliable cache miss detection across different AWS SDK versions

## v2.0.0 - 2025-12-29

This is a major release introducing a new Content-Addressable Storage (CAS) cache system, significant performance improvements, and the migration to Rust 2024 edition.

### Breaking Changes

- **New CAS cache system** - Complete replacement of the legacy tar-based cache with a modern Content-Addressable Storage architecture
  - **Existing caches are incompatible** - All local and remote caches will need to be rebuilt after upgrading
  - Old `.bake/cache` directories can be safely deleted
  - Remote caches (S3/GCS) will start fresh with the new format

### Added

- **Content-Addressable Storage (CAS) cache** - Modern cache architecture with significant improvements
  - Multi-tier storage with local, S3, and GCS backends
  - Content deduplication via content-addressable blob storage - identical files are stored only once
  - Action Cache (AC) for efficient mapping of task inputs to outputs
  - Incremental chunking with content-defined boundaries for better deduplication
  - Zstd compression for reduced storage and faster transfers
  - Cryptographic manifest signing (HMAC-SHA256) for cache integrity verification
  - Layered cache with automatic promotion from remote to local
  - See `docs/CAS_CACHE.md` for detailed architecture documentation

- **CAS configuration options** - New configuration fields for fine-tuning cache behavior
  - `cas.signing_secret_env` - Environment variable name for manifest signing secret
  - `cas.local.max_size` - Maximum size for local CAS storage with LRU eviction
  - `cas.local.algorithm` - Hash algorithm selection (blake3 or sha256)
  - `cas.compression_level` - Zstd compression level (1-22)

### Changed

- **Rust 2024 edition** - Migrated to the latest Rust edition
  - Requires Rust 1.92.0 or later
  - Benefits include improved async ergonomics and faster doctest compilation

- **Project loading performance** - Significant startup time improvements
  - Parallelized helper execution during template rendering
  - Optimized cookbook discovery with lazy loading
  - Reduced memory footprint for large monorepo projects

- **Updated google-cloud-storage** - Migrated to v1.5.0 with new API
  - Uses new googleapis API structure with builder patterns
  - Improved reliability and error handling

### Fixed

- **Structural Handlebars in cookbook discovery** - Fixed parsing of cookbooks that use structural Handlebars
  - Cookbooks with `{{#if}}` blocks around recipe definitions now load correctly
  - New `load_for_discovery` approach properly renders templates before YAML parsing
  - Resolves chicken-and-egg problem with conditional recipe definitions

## v1.1.0 - 2025-10-14

### Added

- **Cache strategy CLI flag** - New `--cache` flag for fine-grained cache control
  - `--cache local-only` - Use only local cache (disables remote)
  - `--cache remote-only` - Use only remote cache (disables local)
  - `--cache local-first` - Check local cache first, then remote (typical default)
  - `--cache remote-first` - Check remote cache first, then local
  - `--cache disabled` - Disable all caching
  - Replaces the need for multiple flags with a single, clear strategy option
  - Directly controls cache order and which backends are active
  - Backward compatible: `--skip-cache` flag still works as before
  - See `docs/guides/caching.md` for detailed usage examples

- **Remote cache enabled field** - New `enabled` field in remote cache configuration
  - Disable remote caching by default in `bake.yml` with `remotes.enabled: false`
  - Enable selectively via CLI flags like `--cache remote-first` or `--cache local-first`
  - Useful for opt-in remote caching workflows (e.g., only use team cache when needed)
  - Defaults to `true` for backward compatibility
  - Example: Configure S3 cache but keep it disabled unless explicitly enabled via CLI

- **Force version override flag** - New `--force-version-override` flag
  - Allows running bake even when project requires a newer version
  - Useful for testing or when you know the version difference is safe
  - Use with caution as it may lead to unexpected behavior with incompatible versions

### Fixed

- **Dependency graph with templates** - Fixed dependency resolution when using recipe templates
  - Ensures template-instantiated recipes properly participate in the dependency graph
  - Resolves issues where template dependencies weren't correctly tracked

## v1.0.1 - 2025-10-10

### Fixed

- **Remote cache error handling** - Improved graceful degradation when remote caches fail
  - Remote cache failures (S3, GCS) no longer print verbose error messages in normal mode
  - Local cache failure still logs warnings as it's more critical
  - Build continues successfully if local cache works but remote caches fail
  - All cache errors are available in debug logs (`RUST_LOG=debug`) for troubleshooting
  - In verbose mode (`--verbose`), concise notifications are shown for remote cache failures
  - Only fails if ALL cache strategies fail to store the recipe output
  - Improves user experience by reducing noise from transient remote cache issues

## v1.0.0 - 2025-10-10

This is the first stable 1.0 release of Bake! 🎉 This milestone reflects the maturity and stability of the codebase, with comprehensive features, excellent test coverage, and production-ready capabilities.

### Added

- **Tags filtering for recipes and cookbooks** - Filter recipe execution by tags for better organization
  - Execute recipes matching specific tags across cookbooks
  - Combine tag filtering with pattern matching for powerful recipe selection
  - Improves workflow organization for large projects with many recipes

- **Custom Handlebars helpers** - Create reusable template helpers with typed parameters and shell execution
  - Define helpers as YAML files in `.bake/helpers/` directory
  - Support typed parameters: string, number, boolean, array, object
  - Optional and required parameters with default values
  - Helper-specific variables and environment variable access
  - Return string or array types
  - Helper results are cached based on rendered script content
  - See `docs/guides/custom-helpers.md` for complete documentation
  - Example helper files in `resources/tests/valid/.bake/helpers/` demonstrate all features

- **Shell command helpers in templates** - Execute shell commands dynamically during template rendering
  - `{{shell 'command'}}` - Execute a command and return trimmed output as a string
  - `{{shell-lines 'command'}}` - Execute a command and return output as an array of lines
  - Commands execute in the cookbook directory (or project directory for project variables)
  - Output is cached during a single bake execution for performance
  - Useful for dynamic cache inputs (e.g., tracking Go dependencies, git-tracked files)
  - Example: `cache.inputs: "{{shell-lines 'git ls-files src/'}}"`
  - Security: Commands execute with same permissions as bake and inherit recipe environment

- **CLI variable overrides** - Override project variables directly from the command line
  - Use `--var key=value` or `-D key=value` flags to override variables at runtime
  - Supports dynamic configuration without modifying project files
  - Useful for CI/CD pipelines and different deployment environments

### Changed

- **Lazy-loading for cookbooks** - Performance optimization for large projects
  - Cookbooks are now loaded minimally during dependency graph construction
  - Full cookbook loading only happens when recipes are actually executed
  - Significantly improves startup time for projects with many cookbooks
  - Reduces memory footprint for large monorepo setups

### Fixed

- **Verbose mode flag handling** - Improved verbose output configuration
  - CLI verbose flag now properly overrides project configuration
  - Better control over logging and debug information

### Technical

- **Code quality improvements** - Applied DRY principles and eliminated duplication
  - Added `cache_file_name()` helper eliminating 10+ duplicate format strings
  - Consolidated test helper functions into shared modules
  - Removed dead code and unused imports
  - Fixed all clippy warnings (empty doc comments, simplified map_or to is_some_and)
  - Net result: +414 insertions, -246 deletions with improved maintainability
  - All 262 tests passing with zero clippy warnings

- **Documentation updates** - Enhanced JSON schemas and documentation for tags support
  - Updated schema definitions to reflect new tagging features
  - Improved inline documentation and code comments

## v0.16.1

### Fixed

- **reservedThreads configuration**: Fixed bugs in `reservedThreads` configuration handling
  - `effective_max_parallel()` now correctly respects both `maxParallel` and `reservedThreads` settings
  - Setting `reservedThreads: 0` now properly allows usage of all available system threads (common in CI environments)
  - Fixed `maxParallel` default calculation to not pre-subtract reserved threads, allowing proper thread allocation
  - Updated comprehensive tests to ensure correct behavior with various configuration combinations

## v0.16.0

### Changed

- **BREAKING: Configuration field naming** - Changed serialization field names from snake_case to camelCase for consistency
  - `max_parallel` → `maxParallel`
  - `reserved_threads` → `reservedThreads`
  - `fast_fail` → `fastFail`
  - `clean_environment` → `cleanEnvironment`
  - This change affects project configuration files (`bake.yml`) and may require updating existing configurations
  - Updated all tests to use the new camelCase field names
  - This aligns with common JSON/YAML conventions and improves consistency across the configuration schema

## v0.15.0

### Fixed

- **Environment variable loading in project configuration** - Fixed issue where environment variables weren't loaded into the template context during project configuration parsing
  - Project configurations can now properly use `{{env.VARIABLE}}` templates in all sections, including config blocks
  - Added `extract_environment_block()` function to extract environment variables from raw YAML before template processing
  - Environment variables are now available during project template rendering, consistent with cookbook behavior
  - Fixes issues with CI/CD configurations that depend on environment variables like `CI_BUILD`
  - Example: `{{#if (eq env.CI_BUILD "true")}}reserved_threads: 0{{/if}}` now works correctly in project config

## v0.14.1

### Fixed

- **Test environment parameter consistency** - Standardized test environment parameters for better test isolation
  - Tests now use `None` for environment parameter unless specifically testing environment override functionality
  - Environment override tests (`test_environment_overrides()`, `test_project_file_template_rendering()`) continue to use specific environment values
  - Improves test predictability and reduces potential side effects between test runs
  - All 194 tests continue to pass with improved test isolation

## v0.14.0

### Added

- **Project file template rendering** - Project files (`bake.yml`) now support full template processing
  - Variables like `{{var.name}}`, `{{env.VAR}}`, and `{{project.root}}` work in project configuration
  - Environment-specific overrides function correctly in project config
  - S3 bucket names, cache paths, and other config values can now use template variables
  - Consistent behavior with cookbook template rendering
  - Example: `bucket: my-cache-{{var.env}}-{{var.region}}` resolves correctly

- **Parallel execution info in verbose output** - Verbose mode now shows parallelism configuration
  - Displays `🔧 Parallel Execution: X threads (system: Y, configured: Z)` at start of execution
  - Shows actual threads used vs system capacity vs configured value
  - Helps with debugging performance and resource usage
  - Only appears when verbose mode is enabled in project config

## v0.13.2

### Fixed

- **Verbose mode precedence** - Fixed verbose mode configuration priority
  - CLI verbose flag now properly overrides project config verbose setting
  - Changed verbose field from `u8` to `Option<bool>` for cleaner semantics
  - When CLI verbose flag is not specified, project config verbose setting is used
  - When CLI verbose flag is specified, it overrides project config setting

## v0.13.1

### Fixed

- **Enhanced error messaging for template resolution failures** - Improved error context when template resolution fails
  - Error messages now include the specific recipe (cookbook:recipe) where template resolution failed
  - Provides better debugging information for template variable issues
  - Helps developers quickly identify which recipe is experiencing template problems

## v0.13.0

### Added

- **Comprehensive documentation restructure** - Complete reorganization of project documentation
  - New structured documentation hierarchy: getting-started, guides, reference, development  
  - 15+ new focused documentation files covering all aspects of Bake
  - Improved navigation and discoverability of documentation content
  - Better separation between user guides and developer documentation

### Changed

- **README.md completely rewritten** - Much more concise and user-focused  
  - Streamlined quick start guide and installation instructions
  - Clear feature highlights with visual formatting
  - Better organized sections for different user needs
  - Reduced from verbose explanations to focused, actionable content

### Removed

- **Legacy documentation files** - Cleaned up outdated documentation structure
  - Removed docs/auto-update.md, docs/configuration.md, docs/recipe-templates.md
  - Removed schemas/README.md and empty CONTRIBUTING.md
  - Content migrated to new structured documentation format

### Technical

- **Development environment improvements**
  - Enhanced VSCode configuration with better debug settings
  - Updated Rust toolchain to 1.87.0
  - Minor code quality improvements and unused variable cleanup
  - Improved test resource configurations

## v0.12.0

### Added

- **Inline variable system** - Major simplification of variable management
  - Variables now defined directly in `bake.yml` and `cookbook.yml` files instead of separate `vars.yml` files
  - Supports `variables` and `overrides` sections for environment-specific configuration
  - Eliminates need for separate variable files while maintaining full functionality
  - Provides cleaner, more maintainable project structure

- **Enhanced render command** - Complete project visualization capability
  - `--render` flag now renders entire project configuration with all variables resolved
  - Shows project info, cookbook details, and recipe definitions with applied templates
  - Provides comprehensive view of project structure for debugging and documentation
  - Replaces template-specific rendering with full project rendering

### Fixed

- **Cache strategy initialization bug** - Fixed "No cache strategy implementation found for local" error
  - Added missing `.default_strategies()` calls in `CacheBuilder` initialization
  - Cache system now properly registers local, S3, and GCS strategies before attempting to build cache
  - Resolves runtime error that prevented bake from executing recipes with cache configuration

### Breaking Changes

- **Variable file format change** - Projects using separate `vars.yml` files need migration
  - Move `vars.yml` contents to `variables` and `overrides` sections in corresponding YAML files
  - `default` section becomes `variables`, `envs` section becomes `overrides`
  - Template rendering command syntax changed from `--render <template>` to `--render` flag

## v0.11.0

### Added

- **Handlebars control structures support** - Enhanced template engine with conditional logic and loops
  - Support for `{{#if}}`, `{{#unless}}`, and `{{#each}}` blocks in recipe templates
  - More powerful template processing for dynamic recipe generation
  - Improved template flexibility for complex cookbook scenarios

- **Recipe template system** - Reusable recipe definitions with typed parameters
  - Template definitions with parameter validation (string, number, boolean, array, object)
  - Template discovery from `.bake/templates/` directories with inheritance support
  - Template instantiation with parameter substitution using Handlebars
  - Eliminates recipe duplication and standardizes patterns across projects

### Technical

- **Library+Binary architecture refactoring** - Major structural improvements for better testability
  - Refactored from binary-only to library+binary crate pattern
  - Created `src/lib.rs` with public API, moved main logic from `src/main.rs`
  - `main.rs` now serves as thin wrapper calling `bake::run().await`
  - Added `[lib]` configuration to `Cargo.toml` for mixed binary/library support
  - Enables proper integration testing and external library usage

- **Comprehensive test coverage expansion** - Dramatically improved test quality and coverage
  - **Overall test coverage**: Improved from 70.34% to 79.01% (+8.67 percentage points)
  - **Unit tests**: Added comprehensive tests for `lib.rs` (52.16% coverage) and `hashing.rs` (97.88% coverage) 
  - **Integration tests**: Restructured from `tests/integration/` to `tests/` root following Rust conventions
  - **Test files**: Added `baker_tests.rs`, `cache_tests.rs`, `project_tests.rs`, `s3_cache_tests.rs`, `template_tests.rs`
  - **Test fixes**: Resolved all compilation errors, API mismatches, and variable loading issues

- **Configuration system modernization** - Enhanced project configuration architecture
  - Improved configuration parsing and validation
  - Better error handling with contextual information
  - Enhanced variable resolution hierarchy

- **Code quality improvements** - Applied modern Rust best practices
  - Resolved all clippy warnings using modern format string syntax (`{var}` instead of `"{}", var`)
  - Applied consistent code formatting with `cargo fmt`
  - Simplified conditional logic and improved readability
  - Enhanced error handling patterns throughout codebase

## v0.10.0

### Added

- **Clean tree-style execution plan display** - Enhanced visual representation of recipe execution with improved formatting
  - Tree-style output shows clear hierarchy and dependencies between recipes
  - Better visual organization of execution flow for complex projects
  - Improved readability when planning and debugging recipe execution
  
- **Configuration rendering flag** - New `--render` flag for displaying resolved project configuration
  - Shows fully processed configuration with all variables resolved
  - Helpful for debugging template variable substitution and configuration issues
  - Displays the final configuration that will be used during execution

### Technical

- **Code organization improvements** - Eliminated duplication in main.rs by extracting helper functions
  - Better separation of concerns and improved maintainability
  - Cleaner main function with extracted utility functions
  - Enhanced code readability and reduced complexity

## v0.9.2

### Fixed

- **S3 cache extraction reliability** - Fixed "incomplete frame" errors during cache archive extraction in CI environments
  - Added proper file flushing with `file.shutdown().await` to ensure complete download before extraction
  - Prevents race condition where cache extraction started before S3 download was fully written to disk
  - Matches the proven fix already implemented in GCS cache strategy
  - Resolves intermittent cache failures in CI pipelines with slower I/O operations

## v0.9.1

### Fixed

- **Enhanced self-update functionality** - Improved package manager detection and permission handling
  - Detects package-managed installations (Homebrew, APT, YUM, Snap, Flatpak) and prevents self-updates
  - Provides helpful guidance on how to update via the appropriate package manager
  - Checks write permissions before attempting updates to provide better error messages
  - Added comprehensive path detection for various package manager installation locations
  - Enhanced symlink resolution for package managers that use symbolic links

### Documentation

- **Publishing guidelines** - Added comprehensive version publishing process documentation to CLAUDE.md
  - Step-by-step release process with semantic versioning guidelines
  - Documentation update requirements and commit conventions
  - Git tagging and branch management instructions

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
