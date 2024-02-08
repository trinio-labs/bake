# 🍪 bake 🍪

Yet another task runner. This time it's tasty.

Bake is a task runner built to be simpler than Make and to satisfy some of the needs of managing dependent build, test
and deploy tasks in complex projects. It's capable of running tasks in parallel as well as caching outputs based on each
recipe's inputs such as files or environment variables.

## Installation

### Homebrew

```sh
brew install bake
```

### Cargo

```sh
cargo install bake
```

## A bake project

A bake project consists of a root `bake.yml` configuration file, [Cookbooks](#cookbooks) and [Recipes](#recipes).
A Cookbook is a collection of Recipes that share some context while each Recipe is a distinct task that can be run
and cached by `bake`.

A typical project looks like this:

```sh

├── foo
│   ├── src
│   │   └── main.rs
│   ├── cargo.toml
│   └── cookbook.yml
├── bar
│   ├── src
│   │   └── index.js
│   ├── package.json
│   └── cookbook.yml
└── bake.yml
```

Bake is able to quickly scan a directory for `cookbook.yml` files to find cookbooks in the project. It then builds a
dependency graph for all recipes and runs them accordingly.

### Cookbooks

Cookbooks contain recipes that usually share the same context. Typically, a cookbook is a package of a monorepo but it
is not restricted to that logical separation.

A cookbook can be configured by a `cookbook.yml` file such as the example below:

```yml
name: foo
recipes:
  build:
    inputs:
      - "./src/**/*.rs"
    outputs:
      - "./target/foo"
    run: |
      echo "Building foo"
      ./build.sh
    dependencies:
      - "test"
      - "bar:build"
  test:
    run: |
      cargo test
    inputs:
      - "./src/**/*.rs"
    outputs:
      - lcov.info
```

A cookbook can contain any number of recipes and in the future will be able to hold common recipe configurations.

### Recipes

As seen above, every recipe, at a minimum, must have a `run` property that defines how to bake it. It can also state which
recipes it depends on by using the recipe's full name or partial if they both belong to the same cookbook. A recipe can also
specify which files should be considered for caching in the property `inputs`. Inputs are configured as glob patterns
relative to the root of the cookbook.

For a more detailed explanation of the configuration files, please see [Configuration](./docs/configuration.md#recipes).

## Baking recipes

By default, bake will run all recipes in all cookbooks if called without any arguments.

If you want to be more granular, you can run `bake` passing a pattern to filter the recipes to run. The pattern is always
in the form `<cookbook>:<recipe>`.

For example, to run the `build` recipe from the `foo` cookbook, run:

```sh
bake foo:build
```

You can also run all recipes in the `foo` cookbook:

```sh
bake foo:
```

Or all recipes named `build` in any cookbook:

```sh
bake :build
```

## Caching

By default, bake caches runs locally in a directory called `.bake/cache`. Bake will use the combined hash of all inputs of
a recipe, the hash of its dependencies and its run command to create a cache key. This allows for recipes to be cached
and only run again if either a dependency or the recipe itself changes. Bake can also be configured to use a remote storage
to cache recipes such as S3 or GCS.

For more information on how to configure caching, please see [Caching](./docs/configuration.md#caching).
