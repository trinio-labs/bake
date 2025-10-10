# Custom Handlebars Helpers

Bake allows you to create custom Handlebars helpers to extend template functionality. These helpers can execute shell commands, transform strings, process data, and more.

## Overview

Custom helpers are defined as YAML files in the `.bake/helpers/` directory of your project. Each helper can:

- Accept typed parameters (string, number, boolean, array, object)
- Define helper-specific variables
- Access environment variables
- Execute shell scripts
- Return strings or arrays
- Cache results for performance

## Creating a Helper

### Basic Structure

Create a YAML file in `.bake/helpers/` with the following structure:

```yaml
name: helper-name
description: What this helper does
returns: string  # or "array"
parameters:
  param1:
    type: string
    required: true
    description: Description of param1
  param2:
    type: number
    required: false
    default: 42
    description: Description of param2
run: |
  # Shell script using {{params.param1}} and {{params.param2}}
  echo "Result"
```

**Important**: The filename must match the helper name (e.g., `helper-name.yml` for a helper named `helper-name`).

### Parameter Types

Helpers support the following parameter types:

- `string` - Text values
- `number` - Numeric values (integers or floats)
- `boolean` - True/false values
- `array` - Lists of values
- `object` - Key-value mappings

### Return Types

- `string` (default) - Returns a single string value
- `array` - Returns multiple lines as an array (one item per line)

## Examples

### Simple String Transformation

**`.bake/helpers/uppercase.yml`:**
```yaml
name: uppercase
description: Convert text to uppercase
returns: string
parameters:
  text:
    type: string
    required: true
    description: The text to convert to uppercase
run: |
  echo "{{params.text}}" | tr '[:lower:]' '[:upper:]'
```

**Usage:**
```yaml
recipes:
  build:
    run: |
      echo "{{uppercase text="hello world"}}"  # Outputs: HELLO WORLD
```

### Helper with Default Values

**`.bake/helpers/repeat.yml`:**
```yaml
name: repeat
description: Repeat text a specified number of times
returns: string
parameters:
  text:
    type: string
    required: true
    description: The text to repeat
  count:
    type: number
    required: false
    default: 1
    description: Number of times to repeat
run: |
  for i in $(seq 1 {{params.count}}); do
    printf "{{params.text}}"
  done
```

**Usage:**
```yaml
recipes:
  build:
    run: |
      echo "{{repeat text="*" count=5}}"  # Outputs: *****
      echo "{{repeat text="X"}}"          # Outputs: X (uses default count=1)
```

### Helper with Variables

**`.bake/helpers/with-vars.yml`:**
```yaml
name: with-vars
description: Use helper-specific variables
returns: string
variables:
  greeting: "Hello"
  punctuation: "!"
parameters:
  name:
    type: string
    required: true
    description: Name to greet
run: |
  echo "{{var.greeting}}, {{params.name}}{{var.punctuation}}"
```

**Usage:**
```yaml
recipes:
  greet:
    run: |
      echo "{{with-vars name="Bake"}}"  # Outputs: Hello, Bake!
```

### Helper with Environment Variables

**`.bake/helpers/with-env.yml`:**
```yaml
name: with-env
description: Echo an environment variable value
returns: string
environment:
  - BUILD_ENV
parameters:
  prefix:
    type: string
    required: false
    default: ""
    description: Prefix to add before the value
run: |
  echo "{{params.prefix}}$BUILD_ENV"
```

**Usage:**
```yaml
recipes:
  build:
    run: |
      export BUILD_ENV=production
      echo "{{with-env prefix="Environment: "}}"  # Outputs: Environment: production
```

### Array Return Type

**`.bake/helpers/list-files.yml`:**
```yaml
name: list-files
description: List files in a directory
returns: array
parameters:
  dir:
    type: string
    required: false
    default: "."
    description: Directory to list files from
run: |
  ls {{params.dir}}
```

**Usage:**
```yaml
recipes:
  list:
    run: |
      {{#each (list-files dir="src")}}
      echo "File: {{this}}"
      {{/each}}
```

## Built-in Helpers

Bake provides several built-in helpers:

### `shell` - Execute Shell Commands

Executes a shell command and returns its output (trimmed).

**Usage:**
```yaml
recipes:
  build:
    run: |
      echo "Git branch: {{shell 'git rev-parse --abbrev-ref HEAD'}}"
      echo "Date: {{shell 'date +%Y-%m-%d'}}"
```

### `shell_lines` - Execute and Split Output

Executes a shell command and returns lines as an array.

**Usage:**
```yaml
recipes:
  process:
    run: |
      {{#each (shell_lines 'ls *.txt')}}
      process-file {{this}}
      {{/each}}
```

## Template Context

Helpers have access to the same template context as recipes:

- `{{project.root}}` - Project root directory
- `{{cookbook.root}}` - Cookbook directory
- `{{var.name}}` - User-defined variables
- `{{env.VAR}}` - Environment variables
- `{{params.name}}` - Helper parameters

## Caching

Helper results are cached based on:
- The rendered script content
- Parameter values
- Variable values
- Environment variable values

This means helpers are only re-executed when their inputs change, improving performance.

## Best Practices

1. **Name files correctly**: The filename must match the helper name
2. **Validate parameters**: Use `required: true` for mandatory parameters
3. **Provide defaults**: Use `default` for optional parameters
4. **Add descriptions**: Document what each parameter does
5. **Keep scripts simple**: Complex logic should be in separate scripts
6. **Use appropriate return types**: Use `array` for multi-line output
7. **Avoid side effects**: Helpers should be idempotent when possible
8. **Use `printf` instead of `echo -n`**: More portable across shells

## Directory Structure

```
.bake/
├── helpers/
│   ├── uppercase.yml
│   ├── repeat.yml
│   ├── with-vars.yml
│   ├── with-env.yml
│   └── list-files.yml
└── templates/
    └── ...
```

## Error Handling

Bake validates helpers at load time:

- Checks that filename matches helper name
- Validates required parameters are provided
- Validates parameter types match definitions
- Reports clear error messages with file locations

If a helper fails during execution, the error includes:
- Helper name
- Parameter values
- Script output
- Exit code

## Complete Example

The Bake test suite includes working examples of custom helpers in `resources/tests/valid/.bake/helpers/`:

- `uppercase.yml` - String transformation
- `repeat.yml` - Parameterized helper with defaults
- `with-vars.yml` - Helper-specific variables
- `with-env.yml` - Environment variable access
- `list-files.yml` - Array return type

These helpers are demonstrated in the `foo:demo-helpers` recipe.

**Run the example:**
```bash
bake -p resources/tests/valid foo:demo-helpers
```

This demonstrates all helper features including parameters, variables, environment access, and return types.
