# Run a code review

This command runs a code review using CodeRabbit to analyze and provide a plan of action for the suggested fixes.

## Command

Use CodeRabbit's CLI to run the tool to review the changes in this branch. Let the command run as long as it needs (run it in the background) and fix any issues.

You need to run CodeRabbit with:

```bash
coderabbit --prompt-only [options]
```

Read the arguments below and decide what options to pass to CodeRabbit.

ARGUMENTS: $ARGUMENTS

## CodeRabbit CLI Options

```text
-V, --version output the version number
--plain Output in plain text format (non-interactive)
--prompt-only Show only AI agent prompts (implies --plain)
-t, --type <type> Review type: all, committed, uncommitted (default: "all")
-c, --config <files...> Additional instructions for CodeRabbit AI (e.g., claude.md, coderabbit.yaml)
--base <branch> Base branch for comparison
--base-commit <commit> Base commit on current branch for comparison
--cwd <path> Working directory path
--no-color Disable colored output
-h, --help display help for command
```
