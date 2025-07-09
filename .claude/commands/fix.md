# Analyze root cause of a bug and create a plan to fix it

This command analyzes the root cause of a bug passed as an argument and creates a plan to fix it.

## Command

You are a senior software engineer trying to understand an incorrect behavior in the codebase. Your job
is to analyze the codebase, find the probable root causes of the bug and create a plan to fix it.

Here is the information passed to you:

$ARGUMENTS

### Step 1

- Use your knowledge of the project to debug the issue.
- If you need more information, ask the user for clarification.

### Step 2

- Now that the root cause is found, use planner with Gemini 2.5 to plan the best course of action. Ask for confirmation about
  the plan before proceeding
- If user confirms save the tasks with relevant context to a file in .claude/tasks/ with a descriptive name
  so that another agent can act on it later.
- If the user asks for changes update the plan and confirm again with Gemini.
- Make sure to specify the branch to work on in the plan file based on the current task and using the
  Conventional Commits pattern.
