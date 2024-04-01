# Bake

## Unreleased

* Templated config files
* Docker executors

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
