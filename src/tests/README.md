# jj-vine Testing Guide

This directory contains tests for jj-vine.

## Test Types

### Unit Tests
Unit tests are located in `src/` files within `#[cfg(test)] mod tests` blocks. They test individual functions and modules in isolation.

Run unit tests:
```bash
cargo test --lib
```

### Integration Tests
Integration tests are located in `tests/` and test the CLI behavior and jj command integration.

Files:
- `integration_tests.rs` - CLI and e2e tests
- `regression_tests.rs` - Tests for specific bugs and edge cases
- `merge_commit_tests.rs` - Tests for merge commit handling
- `deep_merge_test.rs` - Tests for complex merge scenarios

Run integration tests:
```bash
cargo test --test integration_tests
cargo test --test regression_tests
# etc.
```

### GitLab Integration Tests
Real GitLab API integration tests are in `gitlab_integration_tests.rs`. These connect to a real GitLab instance and create/update actual MRs.

## GitLab Integration Test Setup

### Prerequisites
1. A GitLab instance (GitLab.com or self-hosted)
2. A test repository with push access
3. A personal access token with `api` scope
4. SSH access configured (for git push operations)

### Configuration

1. Copy the example environment file:
   ```bash
   cp .env.example .env
   ```

2. Edit `.env` and fill in your values:
   ```bash
   GITLAB_HOST=https://gitlab.com
   GITLAB_PROJECT=your-username/jj-vine-test-repo
   GITLAB_TOKEN=glpat-your-token-here
   ```

3. (Optional) For self-hosted GitLab with custom certificates:
   ```bash
   GITLAB_CA_BUNDLE=/path/to/ca-bundle.pem
   GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS=true
   ```

### Running GitLab Integration Tests

Run all GitLab integration tests:
```bash
cargo test --test gitlab_integration_tests
```

Run a specific test:
```bash
cargo test --test gitlab_integration_tests test_create_simple_mr -- --nocapture
```

### What the Tests Do

The GitLab integration tests:
- Create unique branch names (using UUIDs) to avoid conflicts
- Push branches to your test repository
- Create and update actual MRs via the GitLab API
- Verify MR fields (source branch, target branch, state, etc.)
- Test error handling (invalid tokens, non-existent projects)

### Cleanup

Tests use unique branch names (`jjmrs-test-{uuid}-{name}`) so they don't interfere with each other. MRs created by tests will remain in your test repository for inspection.

If you want to clean up old test branches and MRs, you can do so manually through the GitLab web UI.

## Test Coverage

To run all tests:
```bash
cargo test
```

To run all tests including GitLab integration tests:
```bash
cargo test
```

## Continuous Integration

Currently, GitLab integration tests are not run in CI as they require GitLab credentials. They are intended for local testing and manual verification.
