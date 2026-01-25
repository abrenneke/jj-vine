# Contributing

All contributions extremely welcome! Please feel free to open an issue or pull request.

## Prerequisites

- [`jj`](https://docs.jj-vcs.dev/latest/install-and-setup/)
- [`cargo`](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- [Rust nightly toolchain](https://rust-lang.github.io/rustup/concepts/channels.html)

   ```bash
   rustup toolchain install nightly
   ```
- Optional: [`just`](https://github.com/casey/just#installation) (for raw commands see the [justfile](./justfile))

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

E2E tests are flagged behind the `no-e2e-tests` feature flag. You can run them as well by running:

```bash
just test
```

#### Prerequisites

1. A GitLab instance (GitLab.com or self-hosted)
2. A GitHub instance (GitHub.com or GitHub Enterprise)
3. An Azure DevOps account (dev.azure.com or self-hosted)
3. A test repository with push access
4. A personal access token with `api` scope
5. SSH access configured (for git push operations)
6. Docker installed and running

#### Configuration

1. Copy the example environment file:

   ```bash
   cp .env.example .env
   ```
2. Edit `.env` and fill in your values for GitLab & GitHub.
3. Launch the Forgejo server:

   ```bash
   just start-forgejo
   ```
4. If needed, initialize the Forgejo server (one-time setup).

   - Create your administrator user on the setup page
   - Add your SSH key to the user settings
   - Generate an access token
   - Create a new repository and seed it with an initial commit
   - Set `FORGEJO_PROJECT` and `FORGEJO_TOKEN` in your `.env` file

#### Cleanup

E2E tests *do not currently clean up*. The testing repo will keep all branches and MRs created by tests. You may want to manually reset it from time to time.

#### Continuous Integration

Currently, E2E tests are not run in CI as they require GitLab & GitHub credentials. They are intended for local testing and manual verification.

## Documentation

Documentation is written in [Djot](https://djot.net/) and synced to Markdown using the `just sync-readmes` command.

Hopefully the code forges will support rendering Djot natively in the future.