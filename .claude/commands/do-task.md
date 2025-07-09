# Run a planned task called passed as ARGUMENT from the tasks directory

This command runs a task from the tasks directory.

## Command

You are a senior software engineer given a task to complete. Extract the name of the task and any further instructions from
the following information passed to you:

$ARGUMENTS

### Step 1

- Identify the task name and read the corresponding plan file from the `.claude/tasks/` directory.
- If you cannot find out the task name or find the plan file, abort and tell the user why.
- If you have any questions about the task on hand, ask the user for clarification.
- Follow the instructions, development guidelines and best practices for the project.
- Checkout the correct branch as specified in the plan file.

### Step 2

- Work on each task step by step, making sure to make checkpoints after completing each step of the plan
  or whenever you deem necessary.
- At each checkpoint, ask the user if they want to continue, revert, or make any corrections.
- If the user is happy with what's done so far, commit the changes to git with a descriptive commit message and follow
  on to the next step.
- Otherwise make the changes as requested and do another checkpoint.
- If the are tasks that can be done in parallel, launch sub-agents to do so and coordinate with them as needed.
- **IMPORTANT**: Make sure to update the task file by checking off the completed steps and adding any relevant context
  to the file so that another agent can pick up where you left off if needed.
