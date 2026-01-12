# `jj-vine`

A tool for submitting stacked Pull/Merge Requests from Jujutsu bookmarks.

Currently supports the following code forges:

- GitLab ([gitlab.com](https://gitlab.com) or self-hosted)
- GitHub ([github.com](https://github.com) or GitHub Enterprise)
- [Forgejo](https://forgejo.org) / [Codeberg](https://codeberg.org) / Gitea

*The canonical location for `jj-vine` is [codeberg.org/abrenneke/jj-vine](https://codeberg.org/abrenneke/jj-vine). GitHub is used as a mirror and CI only.*

## Table of Contents

- [Overview](#overview)
- [Main Features](#main-features)
- [Planned Features](#planned-features)
- [Installation](#installation)

  - [Cargo Binstall](#cargo-binstall)
  - [Pre-built Binaries](#pre-built-binaries)
  - [Alias Setup](#alias-setup)
- [Quick Start](#quick-start)
- [Commands](#commands)

  - [submit](#submit)
  - [init](#init)
- [Configuration](#configuration)

  - [Forge-Specific Required Settings](#forge-specific-required-settings)

    - [GitLab](#gitlab)
    - [GitHub](#github)
    - [Forgejo/Codeberg/Gitea](#forgejo-codeberg-gitea)
  - [Common Optional Settings](#common-optional-settings)
- [Stack Visualization](#stack-visualization)
- [Credits](#credits)
- [FAQs](#faqs)

  - [Is this vibe-coded slop?](#is-this-vibe-coded-slop)
  - [Ok, but really?](#ok-but-really)
  - [Why a new project?](#why-a-new-project)
- [Contributing](#contributing)
- [License](#license)

## Overview

As `jj` is so flexible, it can sometimes be tedious to manage pull requests for a `jj` repository. Additionally, many people like the "stacked pull request" workflow, where a tool can manage your stack of pull requests for you, including modifying the base branch, description, and other settings. `jj-vine` aims to smooth out the process of managing pull requests for a `jj` repository.

There are many tools these days that aim to solve this problem, most notably [`jj-spr`](https://github.com/LucioFranco/jj-spr) and [`jj-stack`](https://github.com/keanemind/jj-stack). `jj-vine` has its own preferred workflow and design choices, and is not a direct replacement for these tools. `jj-vine` supports multiple code forges including GitLab, GitHub, and Forgejo.

Major differences:

- `jj-vine` is bookmark-based, rather than change-based. It currently expects you to create and push your bookmarks before submitting them. This means that you can have multiple changes in each pull request.
- `jj-spr` aims to more be a full workflow rather than a lightweight tool. `jj-vine` is less opinionated.
- `jj-vine` is primarily based around the `submit --tracked` command. This submits *all* (your) tracked bookmarks at once. Other tools often require you to submit each bookmark individually. The idea is to simply "sync your current state to the code forge".

## Main Features

- **Stacked pull request creation** - Automatically creates pull requests with correct base branches based on bookmark dependencies
- **Stack visualization** - Adds a navigable stack diagram to pull request descriptions with links to related pull requests
- **Automatic repointing** - Updates pull request base branches when stack structure changes
- **Complex Branching** - Pull requests can branch into other sibling pull requests

## Planned Features

- **Automatic rebasing** - Once a pull request is merged, rebases the stack on top of the trunk 

## Installation

### Cargo Binstall

The preferred way to install `jj-vine` is to use [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
# Will put the binary in $HOME/.cargo/bin
cargo binstall jj-vine
```

### Pre-built Binaries  

Pre-built binaries are available for Linux, macOS, and Windows (ARM64 and x86_64). You can download from the [releases page](https://codeberg.org/abrenneke/jj-vine/releases).

### Alias Setup

You can set up a `jj` alias to make it easier to use `jj-vine`. Add the following to your `~/.config/jj/config.toml` (or run `jj config edit --user`):

```toml
[aliases]
vine = ["util", "exec", "--", "jj-vine"]
```

Then you can use `jj vine` instead of `jj-vine`. You can of course use any alias name you would like (`jj mr` or `jj pr` work well!).

## Quick Start

1. Run `jj vine init` to set up your code forge configuration for your repository. This is stored in `.jj/repo/config.toml`. You may also move any configuration settings to the global config file `~/.config/jj/config.toml`.

    ```bash
    jj vine init
    ```

2. Push up some bookmarks (auto-generated bookmarks work great!)

    ```bash
    jj new main

    jj commit -m "Add feature A"
    # Make some changes
    jj git push -c @-

    jj commit -m "Add feature B"
    # Make some changes
    jj git push -c @-
    ```

3. Submit all tracked bookmarks at once:

    ```bash
    jj vine submit --tracked
    ```

    This creates two pull requests:
    
    - `feature-a` targeting `main`
    - `feature-b` targeting `feature-a`

## Commands

### `submit`

Submit a bookmark and its dependencies as pull requests.

```bash
# Submit a single bookmark (and its dependencies)
jj vine submit <bookmark>

# Submit all tracked bookmarks
jj vine submit --tracked

# Preview without making changes
jj vine submit <bookmark> --dry-run
```

See all options with `jj vine submit --help`.

### `init`

Interactive setup wizard to configure jj-vine for your repository.

```bash
jj vine init
```

## Configuration

Configuration is stored in jj's config system under the `jj-vine` section. You can use `jj config edit --repo` to edit the configuration for a specific repository. You can also use `jj config set --repo <key> <value>` to set a configuration value for a specific repository:

```bash
jj config set --repo jj-vine.deleteSourceBranch true
jj config set --repo jj-vine.defaultReviewers '["alice", "bob"]'
```

### Forge-Specific Required Settings

#### GitLab

Enabled when `jj-vine.forge` is set to `gitlab`.

| Setting | Description |
|---------|-------------|
| `gitlab.host` | GitLab instance URL (e.g., `https://gitlab.example.com`) |
| `gitlab.project` | Project ID where branches are pushed (`group/project` or numeric ID like `12345`) |
| `gitlab.token` | Personal Access Token with `api` scope |
| `gitlab.target_project` | *(Optional)* Target project ID for merge requests (e.g., `upstream/project`). If set and different from `project` (forks). |

#### GitHub

Enabled when `jj-vine.forge` is set to `github`.

| Setting | Description |
|---------|-------------|
| `github.host` | GitHub API URL (defaults to `https://api.github.com` for GitHub.com, or `https://github.example.com/api/v3` for Enterprise) |
| `github.project` | Repository where branches are pushed in `owner/repo` format |
| `github.token` | Personal Access Token with `repo` scope |
| `github.target_project` | *(Optional)* Target repository for pull requests (e.g., `upstream-owner/repo`). If set and different from `project` (forks). |

#### Forgejo/Codeberg/Gitea

Enabled when `jj-vine.forge` is set to `forgejo`.

| Setting | Description |
|---------|-------------|
| `forgejo.host` | Forgejo/Codeberg/Gitea instance URL (e.g., `https://codeberg.org`) |
| `forgejo.project` | Repository where branches are pushed in `owner/repo` format |
| `forgejo.token` | API access token with `repo` scope |
| `forgejo.target_project` | *(Optional)* Target repository for pull requests (e.g., `upstream-owner/repo`). If set and different from `project` (forks). |

### Common Optional Settings

These settings apply to all forges:

| Setting | Default | Description |
|---------|---------|-------------|
| `remoteName` | `origin` | Git remote name |
| `defaultBranch` | `main` | Default target branch for PRs/MRs |
| `deleteSourceBranch` | `true` | Auto-delete source branch when merged |
| `squashCommits` | `false` | Squash commits when merging |
| `assignToSelf` | `false` | Automatically assign PRs/MRs to creator |
| `defaultReviewers` | `[]` | List of usernames to add as reviewers |
| `enableStackVisualization` | `true` | Include stack diagram in PR/MR descriptions |
| `stackFormat` | `linear` | Stack visualization format |
| `caBundle` | - | Path to CA certificate bundle for custom TLS |
| `tlsAcceptNonCompliantCerts` | `false` | Accept non-standard TLS certificates |

## Stack Visualization

When enabled, jj-vine adds a stack diagram to pull request descriptions:

```markdown
<!-- start jj-vine stack -->
This MR is part of a stack of 3 MRs:

1. Base PR Title - !123
2. **Feature A ← this MR**
3. Feature B - !125
<!-- end jj-vine stack -->
```

If a pull request has multiple children, it will be listed multiple times in the stack diagram:

```markdown
<!-- start jj-vine stack -->
This MR is part of 3 stacks:

Stack 1 (2 MRs):
1. **Feature A ← this MR**
2. Feature B - !125

Stack 2 (2 MRs):
1. **Feature A ← this MR**
2. Feature B - !126

Stack 3 (2 MRs):
1. **Feature A ← this MR**
2. Feature B - !127
<!-- end jj-vine stack -->
```

User content added before or after these markers is preserved on re-submission.

You may disable the stack visualization by setting `jj-vine.enableStackVisualization` to `false` in your configuration. If disabled, pull request descriptions will not be touched.

## Credits

- [`jj-spr`](https://github.com/LucioFranco/jj-spr) heavily for inspiration & code approaches
- [`jj-stack`](https://github.com/keanemind/jj-stack) heavily for inspiration & code approaches

## FAQs

### Is this vibe-coded slop?

Don't worry, I berated Claude with profanity until things looked good.

### Ok, but really?

This started as a test to see if Claude Code was an effective tool. Conclusion: meh. It made so many mistakes and bad decisions and I had to babysit it the entire time. A lot of code is AI-written, but I've done plenty of refactoring and cleanup myself. There are a decent amount of tests verifying the behavior. Lots of profanity really was involved.

### Why a new project?

Well primarily, existing tools did not support GitLab (though `jj-vine` now supports GitLab, GitHub, and Forgejo). `jj-spr` was too heavy-handed. `jj-stack` was in TypeScript (nothing against it, but seems sane for a `jj` tool to also be built in Rust). My current `jj` workflow was also just different enough that those existing tools did not fit my needs.

## Contributing

All contributions extremely welcome! Please feel free to open an issue or pull request. See [CONTRIBUTING.djot](./CONTRIBUTING.djot) for more details.

## License

[MIT License](./LICENSE)
