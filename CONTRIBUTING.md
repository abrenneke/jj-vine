# Contributing

All contributions extremely welcome! Please feel free to open an issue or pull request.

## Prerequisites

- [`jj`](https://docs.jj-vcs.dev/latest/install-and-setup/)
- [`cargo`](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- [Rust nightly toolchain](https://rust-lang.github.io/rustup/concepts/channels.html)

   ```bash
   rustup toolchain install nightly
   ```
- Optional: [`just`](https://github.com/casey/just#installation) (for raw commands see [justfile](./justfile))

## Basic Commands

### Build, Lint, and Test

```bash
just
```

### Building

```bash
just build
```

You will need the nightly toolchain to build. Why? Only for a couple `rustfmt` features :(

### Formatting & Linting

```bash
just lint
```

## Testing

### Unit Tests

```bash
just test-unit
```

### Integration Tests

Integration tests are located in `src/tests/` and test the CLI behavior and jj command integration. The integration test suite require GitLab credentials to run.

Run full test suite including integration tests:

```bash
just test
```

#### Prerequisites

1. A GitLab instance (GitLab.com or self-hosted)
2. A test repository with push access
3. A personal access token with `api` scope
4. SSH access configured (for git push operations)

#### Configuration

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

#### Cleanup

GitLab integration tests *do not currently clean up*. The testing repo will keep all branches and MRs created by tests. You may want to manually reset it from time to time.

#### Continuous Integration

Currently, integration tests are not run in CI as they require GitLab credentials. They are intended for local testing and manual verification.

## Documentation

Documentation is written in [Djot](https://djot.net/) and synced to Markdown using the `just sync-readmes` command.

Hopefully the code forges will support rendering Djot natively in the future.