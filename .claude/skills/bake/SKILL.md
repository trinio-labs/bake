---
name: bake
description: Use when this repository relies on Bake as its task runner. Prefer Bake recipes over raw cargo, go, npm, or other tool commands when a matching recipe exists. Read the project-specific Bake reference before choosing commands.
---

# Bake

Treat Bake as the canonical task runner for this repository.

## Workflow

- Read [project-specific-skill.md](references/project-specific-skill.md) before suggesting or running commands.
- If the repository defines a Bake recipe for a task such as build, test, lint, or release, prefer the Bake recipe over the underlying tool command.
- For example, if the reference shows a test recipe for a cookbook or domain, use `bake <cookbook>:test` instead of jumping straight to `cargo test`, `go test`, `npm test`, or similar raw commands.
- Bake resolves recipe dependencies automatically. If `bake app:test` depends on `app:build` or setup recipes, select `app:test` and let Bake schedule the prerequisites.
- Do not manually run prerequisite Bake recipes before the target unless the user explicitly asks for that narrower step.
- Prefer targeted selectors: `bake <cookbook>:<recipe>` for one recipe, `bake <cookbook>:` for one cookbook scope, and `bake :<recipe>` for the same recipe across multiple cookbooks.
- Prefer `bake --show-plan ...` before broad or unfamiliar runs.
- Use `bake --dry-run` when the user wants a preview without execution.
- Use `--env <name>`, `-D key=value`, `--tags`, and `--regex` only when they materially change the selection.
- Fall back to raw tool commands only when no suitable Bake recipe exists or the user explicitly asks to bypass Bake.
- If you update Bake cookbooks, recipes, templates, or helpers, regenerate this skill with `bake --generate skill`.

## Reference

- For repository-specific cookbook names, recipes, environments, templates, helpers, and examples, read [project-specific-skill.md](references/project-specific-skill.md).
