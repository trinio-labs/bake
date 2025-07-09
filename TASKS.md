# Main.rs Code Quality Improvement Tasks

## Code Review Summary - main.rs only

Based on comprehensive code review focusing on security checks (OWASP Top 10), code smells, and architectural best practices.

### Issues Found:
- **HIGH**: Panic risks from unwrap() usage (lines 119, 159, 194)
- **MEDIUM**: Overly long main() function (180+ lines, violates SRP)
- **MEDIUM**: Path security concerns (potential traversal vulnerabilities)
- **LOW**: Code duplication in path resolution (lines 118-122, 193-197)
- **LOW**: Hard-coded values (magic string "warn" at line 94)

## Implementation Plan

### Phase 1: Safety & Foundation [HIGH PRIORITY]

#### Task 1.1: Remove unwrap() calls and improve error handling
- [ ] Replace `std::env::current_dir().unwrap()` with `std::env::current_dir()?` at lines 119, 194
- [ ] Replace `tokio::spawn_blocking().await.unwrap()` with proper error handling at line 159
- [ ] Add descriptive error messages where needed
- [ ] Test: Verify application doesn't panic on permission errors

#### Task 1.2: Create path resolution helper function
- [ ] Extract duplicated path logic from lines 118-122 and 193-197
- [ ] Create `resolve_bake_path(path_arg: &Option<String>) -> anyhow::Result<PathBuf>`
- [ ] Include path validation and canonicalization
- [ ] Test: Verify both relative and absolute paths work correctly

#### Task 1.3: Define constants for magic values
- [ ] Replace hard-coded "warn" with `DEFAULT_LOG_LEVEL` constant
- [ ] Consider other magic values that should be constants
- [ ] Test: Verify logging behavior remains unchanged

### Phase 2: Architectural Refactoring [MEDIUM PRIORITY]

#### Task 2.1: Extract command handlers from main()
- [ ] Create `handle_update_info()` function for --update-info flag
- [ ] Create `handle_update_version()` function for --update-version flag
- [ ] Create `handle_self_update()` function for --self-update flag
- [ ] Create `handle_check_updates()` function for --check-updates flag
- [ ] Create `run_bake()` function for main recipe execution logic
- [ ] Update main() to dispatch to appropriate handlers
- [ ] Test: Verify all command-line options work identically

#### Task 2.2: Implement secure path validation
- [ ] Add path traversal protection to `resolve_bake_path()`
- [ ] Implement path sanitization and bounds checking
- [ ] Add security validation for configuration file paths
- [ ] Test: Verify protection against directory traversal attacks

#### Task 2.3: Optimize error handling flow
- [ ] Ensure consistent error handling across all handlers
- [ ] Add context-specific error messages
- [ ] Validate error propagation works correctly
- [ ] Test: Verify error messages are helpful and appropriate

### Phase 3: Integration & Validation [LOW PRIORITY]

#### Task 3.1: Final integration testing
- [ ] Run complete test suite to verify all changes work together
- [ ] Test edge cases and error conditions
- [ ] Validate performance hasn't degraded
- [ ] Test: Complete regression testing

#### Task 3.2: Code quality validation
- [ ] Run `cargo clippy` to check for linting issues
- [ ] Run `cargo fmt` to ensure consistent formatting
- [ ] Review code for adherence to project standards
- [ ] Test: Verify build pipeline passes

#### Task 3.3: Documentation and cleanup
- [ ] Update any inline documentation if needed
- [ ] Clean up temporary files and unused code
- [ ] Verify all constants and helpers are properly documented
- [ ] Test: Final functionality verification

## Execution Strategy

1. Work through tasks sequentially within each phase
2. Run tests after each task completion
3. Stop after each task to get user approval before proceeding
4. Maintain rollback capability at each step
5. Track progress in this file

## Progress Tracking

- [ ] Phase 1: Safety & Foundation
- [ ] Phase 2: Architectural Refactoring
- [ ] Phase 3: Integration & Validation

---
*Generated from code review on 2025-07-09*