# Run a code review with $ARGUMENTS

This command runs a code review using Gemini 2.5 Pro to analyze and provide a plan of action for the suggested fixes.

## Command

You are managing a code review for this project. Follow the following steps to perform this task:

### Step 1

Use zen's codereview tool to ask Gemini 2.5 Pro to review using the following
parameters:

- **Arguments**: $ARGUMENTS
- **Checks**: Unless stated otherwise in ARGUMENTS always run the following checks:
  - Security checks looking for OWASP Top 10 vulnerabilities
  - Code smells
  - Architectural best practices

### Step 2

Use the output from the codereview tool to create a plan of action for the suggested fixes using zen's planner tool. Ask
for confirmation about the plan before proceeding.

- If user confirms save the tasks with relevant context to a file in .claude/tasks/ with a descriptive name
  so that another agent can act on it later.
- If the user ask for changes update the plan and confirm again with Gemini.
- Make sure to include a final task to run the codereview tool again after the fixes are applied to ensure that the
  issues have been resolved.
- Make sure to specify the branch to work on in the plan file based on the current task and using the
  Conventional Commits pattern.
