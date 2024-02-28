# Bake

## Unreleased

* Templated config files
* Project, cookbook and recipe variables
* Docker executors
* Change output and inputs to cache property in recipe config

## v0.2.1

Adds a way to pass variables via command line.

### Added

* Documentation
* Command line option to override project variables

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
