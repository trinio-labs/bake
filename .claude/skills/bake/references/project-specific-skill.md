# Project-Specific Bake Reference

This repository uses Bake to define project tasks in `bake.yml` and cookbook files. A cookbook is the project-local scope or domain used in selectors like `foo:build`.

Project: `bake`. Dogfood Bake against the Bake repository itself

## Targeting Recipes
- `bake repo:build` runs a single recipe inside one cookbook.
- `bake repo:` runs every recipe in that cookbook.
- `bake :<recipe>` runs the same recipe across every cookbook that defines it.
- Bake resolves declared dependencies automatically, so selecting a target recipe also schedules its prerequisite recipes.
- Add `--regex` to treat both sides of `cookbook:recipe` as regular expressions.
- Add `--tags <tag1,tag2>` to match recipes that carry any of those tags.
- Use `--show-plan`, `--dry-run`, `--clean`, `--env <name>`, and `-D key=value` when you need planning, cleanup, environment selection, or variable overrides.

## Project Inventory
- Project name: `bake`
- Cookbooks: `repo`
- Project environment variables exposed to recipes: `CI`, `RUST_LOG`, `CARGO_TERM_COLOR`

## Cookbook Scopes

### `repo`
Inherited cookbook tags: `rust`, `dogfood`
- `repo:build`: Build Bake. tags: `rust`, `dogfood`
- `repo:check`: Run the core dogfood checks for Bake. tags: `rust`, `dogfood`. depends on `repo:build`, `repo:fmt`, `repo:lint`, `repo:test-lib`, `repo:test-integration`
- `repo:fmt`: Check Rust formatting. tags: `rust`, `dogfood`
- `repo:lint`: Run clippy for Bake. tags: `rust`, `dogfood`
- `repo:test-integration`: Run Bake integration tests. tags: `rust`, `dogfood`
- `repo:test-lib`: Run Bake library tests. tags: `rust`, `dogfood`
