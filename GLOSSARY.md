# Bake

Bake is a parallel task runner that caches recipe results by hashing their inputs. These are the terms its specs and docs rely on.

## Caching

**Local cache**:
The on-disk cache bake reads and writes on this machine, holding stored blobs and the results recorded for each cache key.
_Avoid_: CAS cache, disk cache

**Cache key**:
The hash identifying one recipe result, derived from the recipe's inputs, command, variables and dependencies.
_Avoid_: Action key, recipe hash

## Git worktrees

**Main worktree**:
The working tree that holds a repository's `.git` directory.
_Avoid_: Primary checkout, root repo

**Linked worktree**:
An additional working tree of the same repository, created with `git worktree add`, often one per coding agent.
_Avoid_: Secondary checkout, agent worktree
