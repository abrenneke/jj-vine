# `jj-vine`

A tool for submitting stacked Pull/Merge Requests from Jujutsu bookmarks.

Supports the following code forges:

- GitLab ([gitlab.com](https://gitlab.com) or self-hosted)
- [GitHub](https://github.com) or GitHub Enterprise
- [Forgejo](https://forgejo.org) / [Codeberg](https://codeberg.org) / Gitea
- [Azure DevOps](https://azure.microsoft.com/en-us/products/devops)

*The canonical location for `jj-vine` is [codeberg.org/abrenneke/jj-vine](https://codeberg.org/abrenneke/jj-vine). GitHub is used as a mirror and CI only.*

## Table of Contents

- [Overview](#overview)
- [Main Features](#main-features)
- [Planned Features](#planned-features)
- [Installation](#installation)

  - [Cargo Binstall](#cargo-binstall)
  - [Mise](#mise)
  - [Pre-built Binaries](#pre-built-binaries)
  - [Attestations](#attestations)
  - [Alias Setup](#alias-setup)
- [Quick Start](#quick-start)
- [Commands](#commands)

  - [submit](#submit)
  - [init](#init)
  - [status](#status)
- [Configuration](#configuration)

  - [Forge-Specific Settings](#forge-specific-settings)

    - [Forge](#forge)
    - [GitLab](#gitlab)
    - [GitHub](#github)
    - [Forgejo/Codeberg/Gitea](#forgejo-codeberg-gitea)
    - [Azure DevOps](#azure-devops)
  - [Common Settings](#common-settings)
- [Description Generation / Stack Visualization](#description-generation--stack-visualization)

  - [Configuration](#configuration)
  - [Linear Format](#linear-format)
  - [Tree Format](#tree-format)
- [Credits](#credits)
- [FAQs](#faqs)

  - [Is this vibe-coded slop?](#is-this-vibe-coded-slop)
  - [Ok, but really?](#ok-but-really)
  - [Why a new project?](#why-a-new-project)
- [Contributing](#contributing)
- [License](#license)

## Overview

As `jj` is so flexible, it can sometimes be tedious to manage pull requests for a `jj` repository. Additionally, many people like the "stacked pull request" workflow, where a tool can manage your stack of pull requests for you, including modifying the base branch, description, and other settings. `jj-vine` aims to smooth out the process of managing pull requests for a `jj` repository.

There are many tools these days that aim to solve this problem, most notably [`jj-spr`](https://github.com/LucioFranco/jj-spr) and [`jj-stack`](https://github.com/keanemind/jj-stack). `jj-vine` has its own preferred workflow and design choices, and is not a direct replacement for these tools. `jj-vine` supports multiple code forges: GitLab, GitHub, Forgejo, and Azure DevOps (including self-hosted instances).

Major differences:

- `jj-vine` is bookmark-based, rather than change-based. It expects you to create your bookmarks before submitting them, and usually expects you to push them (e.g. `jj git push -c @`) as well. This means that you can have multiple commits in each pull request.
- `jj-spr` aims to more be a full workflow rather than a lightweight tool. `jj-vine` is less opinionated.
- `jj-vine` is primarily based around the `submit --tracked` command. This submits *all* (your) tracked bookmarks at once. Other tools often require you to submit each bookmark individually. The idea is to simply "sync your current state to the code forge".

## Main Features

- **Stacked pull/merge request creation**

    Automatically creates pull/merge requests with correct base branches based on bookmark dependencies.
- **Stack visualization**

    Adds a navigable stack diagram to pull/merge request descriptions with links to related pull/merge requests. Can be customized and disabled.
- **Unopinionated**

    `jj-vine` is not opinionated about how you should structure your commits, bookmarks, and pull/merge requests. It can work with what you have. It also works great with auto-generated bookmarks.
- **Complex branching**

    Not only does `jj-vine` support trees of bookmarks, but it fully supports complex (DAG) graphs of changes just like `jj` itself does. While actual forge support for PRs/MRs with multiple parents may be limited, `jj-vine` can still visualize and manage them.
- **Status**

    Can easily report the status of your bookmarks and their pull/merge requests.
- **Automatic syncing**

    Updates pull/merge request base branches & all related descriptions when stack structure changes.

## Planned Features

- **Automatic rebasing**

    Once a pull/merge request is merged, rebases the stack on top of the trunk
- **Landing**

    Merge a pull request and automatically rebase dependent pull/merge requests on top of the trunk

## Installation

### Cargo Binstall

The preferred way to install `jj-vine` is to use [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
# Will put the binary in $HOME/.cargo/bin
cargo binstall jj-vine
```

### Mise

If you use [mise](https://mise.jdx.dev/), you can install with:

```bash
mise use -g cargo:jj-vine
```

### Pre-built Binaries

Pre-built binaries are available for Linux, macOS, and Windows (ARM64 and x86_64 for all). You can download directly from the [releases page](https://codeberg.org/abrenneke/jj-vine/releases).

### Attestations

Binaries are built with GitHub attestations. You may [verify the provenance of a binary](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations#verifying-artifact-attestations-with-the-github-cli) by running:

```bash
gh attestation verify <binary> -R abrenneke/jj-vine
```

### Alias Setup

You can set up a `jj` alias to make it easier to use `jj-vine`. Aliases that work great are `jj pr`, `jj mr`, or `jj vine`. You can run the following command to install an alias:

```bash
# Take your pick:
jj config set --user aliases.pr '["util", "exec", "--", "jj-vine"]'
jj config set --user aliases.mr '["util", "exec", "--", "jj-vine"]'
jj config set --user aliases.vine '["util", "exec", "--", "jj-vine"]'
```

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
# Submit a single bookmark or revset (and its dependencies!)
jj vine submit <revset/bookmark>
jj vine submit -r <revset/bookmark>

# Submit all tracked bookmarks. Roughly equivalent to `(mine() & tracked_remote_bookmarks()) ~ trunk()`, but has additional stipulations. See `jj-vine submit --help` for more details.
# This is the recommended command to use!
jj vine submit --tracked

# Preview without making changes
jj vine submit <options> --dry-run
```

See all options and additional help with `jj-vine submit --help`.

### `init`

Interactive setup wizard to configure jj-vine for your repository.

```bash
jj vine init
```

### `status`

Show the status of tracked bookmarks and their pull/merge requests.

```bash
# Show the status of all my bookmarks
jj vine status

# Show the status of all tracked bookmarks
jj vine status --tracked

# Show the status of a specific revset that includes bookmarks
jj vine status -r <revset>
```

The output will look roughly like this (but with colors):

```bash
!100 "This is the title of the first merge request"
     push-xxxxxxxa • ✓ Checks OK • Needs approval (0/1) • 2 open discussions • 24d 21h old • https://forge-url.example/100

!101 "This is the title of the second merge request"
     push-xxxxxxxb • [READY] • ✓ Checks OK • Approved (1/1) • 16d 18h old • https://forge-url.example/101

!102 "Third title"
     push-xxxxxxxc • ✓ Checks OK • Needs approval (0/1) • 5d 6h old • https://forge-url.example/102

!103 "Fourth merge request title"
     push-xxxxxxxd • ✓ Checks OK • Needs approval (0/1) • 19h old • https://forge-url.example/103

!104 "Fifth title"
     push-xxxxxxxe • ✗ Checks failing • Needs approval (0/1) • 1 open discussion • 19h old • https://forge-url.example/104

wip No merge request

wip-2 No merge request
```

See all options and additional help with `jj-vine status --help`.

## Configuration

Configuration is stored in [jj's configuration system](https://docs.jj-vcs.dev/latest/config/) under the `jj-vine` section. You can use `jj config edit --repo` to edit the configuration for a specific repository or `jj config edit --user` to edit the global configuration. You can also use `jj config set --repo <key> <value>` to set a configuration value for a specific repository:

```bash
jj config set --repo jj-vine.deleteSourceBranch true
jj config set --repo jj-vine.defaultReviewers '["alice", "bob"]'
```

### Forge-Specific Settings

All listed settings are under the `jj-vine` section so should be prefixed with `jj-vine.`.

#### Forge

Ad a minimum, the `jj-vine.forge` configuration setting must be set to the type of forge you are using.

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `forge` | The type of forge you are using | "gitlab" \| "github" \| "forgejo" \| "azure" | Yes | - |

#### GitLab

Required when `jj-vine.forge` is set to `gitlab`.

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `gitlab.host` | GitLab instance URL (e.g., `https://gitlab.example.com`) | String | Yes | - |
| `gitlab.project` | Project ID where branches are pushed (`group/project` or numeric ID like `12345`) | String | Yes | - |
| `gitlab.token` | Personal Access Token with `api` scope | String | Yes | - |
| `gitlab.targetProject` | Target project ID for merge requests (e.g., `upstream/project`). Use if you are using a fork. | String | No | (same as `gitlab.project`) |
| `gitlab.createMergeRequestDependencies` | Whether to create dependencies between merge requests, requiring that all parent merge requests are merged before the child merge request can be merged. | Boolean | No | true |

#### GitHub

Required when `jj-vine.forge` is set to `github`.

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `github.host` | GitHub API URL (defaults to `https://api.github.com` for GitHub.com, or `https://github.example.com/api/v3` for Enterprise) | String | Yes | - |
| `github.project` | Repository where branches are pushed in `owner/repo` format | String | Yes | - |
| `github.token` | Personal Access Token with `repo` scope | String | Yes | - |
| `github.targetProject` | Target repository for pull requests (e.g., `upstream-owner/repo`). Use if you are using a fork. | String | No | (same as `github.project`) |

#### Forgejo/Codeberg/Gitea

Required when `jj-vine.forge` is set to `forgejo`.

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `forgejo.host` | Forgejo/Codeberg/Gitea instance URL (e.g., `https://codeberg.org`) | String | Yes | - |
| `forgejo.project` | Repository where branches are pushed in `owner/repo` format | String | Yes | - |
| `forgejo.token` | API access token with `repo` scope | String | Yes | - |
| `forgejo.targetProject` | Target repository for pull requests (e.g., `upstream-owner/repo`). Use if you are using a fork. | String | No | (same as `forgejo.project`) |
| `forgejo.wipPrefix` | Prefix for WIP/draft pull requests. What counts as a draft pull request is configurable per-repository on Forgejo. | String | No | "WIP: " |

#### Azure DevOps

Required when `jj-vine.forge` is set to `azure`.

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `azure.host` | Azure DevOps instance URL (e.g., `https://dev.azure.com`) | String | Yes | - |
| `azure.vsspsHost` | Azure DevOps Security (VSSP) host (e.g., `https://vssps.dev.azure.com`). Used to look up other users for automatic review requests. | String | No | - |
| `azure.project` | Organization and project where branches are pushed, formatted as `organization/project` | String | Yes | - |
| `azure.sourceRepositoryName` | Name of the repository in the project where branches are pushed | String | Required if `azure.sourceRepositoryId` is not set | - |
| `azure.sourceRepositoryId` | ID of the repository in the project where branches are pushed | String | Required if `azure.sourceRepositoryName` is not set | - |
| `azure.token` | Personal Access Token | String | Yes | - |
| `azure.targetProject` | Target organization & project for pull requests (e.g., `upstream-organization/project`). Use if you are using a fork. | String | No | (same as `azure.project`) |
| `azure.targetRepositoryName` | Name of the repository in the target project for pull requests | String | Required if `azure.targetRepositoryId` is not set and `azure.targetProject` is different from `azure.project` | - |
| `azure.targetRepositoryId` | ID of the repository in the target project for pull requests | String | Required if `azure.targetRepositoryName` is not set and `azure.targetProject` is different from `azure.project` | - |

### Common Settings

These settings apply to all forges:

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `remoteName` | The remote name to use for pushing and pulling branches | String | No | "origin" |
| `deleteSourceBranch` | Configures the pull/merge request to delete the source branch when merged. Currently has no effect for GitHub and Forgejo (is a repository-level setting and on-merge flag only) | Boolean | No | true |
| `squashCommits` | Configures the pull/merge request to squash commits when merging. Currently has no effect for GitHub and Forgejo (is a repository-level setting and on-merge flag only) | Boolean | No | false |
| `assignToSelf` | Automatically assign created pull/merge requests to yourself. Has no effect for AzureDevOps | Boolean | No | false |
| `defaultReviewers` | List of usernames to automatically add as reviewers when creating pull/merge requests. For Azure DevOps, this should be a list of "user descriptors" and `azure.vsspsHost` must be set | Array<String> | No | [] |
| `caBundle` | Path to CA certificate bundle for custom TLS | String \| null | No | null |
| `tlsAcceptNonCompliantCerts` | Accept non-compliant TLS certificates (for certificates that don't meet strict X.509 standards). This is almost always unnecessary unless you have a unique situation. | Boolean | No | false |
| `defaultBaseBranch` | Default target branch for pull/merge requests into `trunk()` | String | No | (detected automatically using the `trunk()` revset) |
| `openAsDraft` | Open newly created pull/merge requests as drafts | Boolean | No | false |
| `description` | Configuration for pull/merge request description generation | Object (see below) | No | (see below) |

## Description Generation / Stack Visualization

`jj-vine` can generate stack diagrams for pull/merge requests and add them to the descriptions of pull/merge requests. When enabled,
every time you `submit` your bookmark(s), the descriptions for all impacted pull/merge requests will be updated as well.

### Configuration

| Setting | Description | Type | Required | Default |
|---------|-------------|------|----------|---------|
| `description.enabled` | Whether to enable or disable description generation entirely. If false, pull/merge request descriptions will not be touched | Boolean | No | true |
| `description.format` | How to render the description for different types of merge request stacks | Object | No | (see next rows) |
| `description.format.single` | How to render a **single** pull/merge request, without any parents or children besides the trunk | "none" \| "linear" \| "tree" | No | "none" |
| `description.format.linear` | How to render a **linear** stack of bookmarks. This means that no tracked bookmark has multiple parents or multiple children | "none" \| "linear" \| "tree" | No | "linear" |
| `description.format.tree` | How to render a **tree** of bookmarks, where two bookmarks merge into a common parent, but no bookmark has multiple parents | "none" \| "linear" \| "tree" | No | "tree" |
| `description.format.complex` | How to render a **complex** (DAG) graph of bookmarks, where any bookmark has multiple parents. Because forges only support a pull/merge request merging into a single parent, in this situation you may see commits of one pull/merge request included in other pull/merge requests | "none" \| "linear" \| "tree" | No | "complex" |

The following sections show examples of the different formats.

### Linear Format

#### Linear/Single Bookmark Stack

(`description.format.single = "linear"` and `description.format.linear = "linear"`)

This PR is part of a stack containing 5 PRs:

1. `main`
2. [#1](#) "Feature A"
3. **"Feature B" ← this PR**
4. [#3](#) "Feature C"
5. [#4](#) "Feature D"
6. [#5](#) "Feature E"

#### Tree Bookmarks

(`description.format.tree = "linear"`)

This PR is part of a tree containing 8 PRs:

1. `main`
2. [#1](#) "Feature A" → `main`
3. [#4](#) "Feature D" → [#1](#)
4. [#2](#) "Feature B" → [#1](#)
5. **"Feature E" → [#2](#) ← this PR**
6. [#3](#) "Feature C" → [#2](#)
7. [#7](#) "Feature G" → [#3](#)
8. [#8](#) "Feature H" → [#7](#)
9. [#6](#) "Feature F" → [#3](#)

#### Complex Graph of Bookmarks

(`description.format.complex = "linear"`)

This PR is part of a complex set of PRs containing 10 PRs:

1. `main`
2. [#9](#) "Feature I" → `main`
3. [#10](#) "Feature J" → [#9](#)
4. [#1](#) "Feature A" → `main`
5. [#2](#) "Feature B" → [#1](#)
6. **"Feature E" → [#2](#), [#10](#) ← this PR**
7. [#4](#) "Feature D" → [#1](#), [#2](#)
8. [#3](#) "Feature C" → [#2](#)
9. [#7](#) "Feature G" → [#3](#), [#5](#), [#10](#)
10. [#8](#) "Feature H" → [#7](#)
11. [#6](#) "Feature F" → [#3](#), [#9](#)

### Tree Format

#### Linear/Single Bookmark Stack

(`description.format.single = "tree"` and `description.format.linear = "tree"`)

This PR is part of a stack containing 5 PRs:

- `main`

    - [#1](#) "Feature A"

        - **"Feature B" ← this MR**

            - [#3](#) "Feature C"

                - [#4](#) "Feature D"

                    - [#5](#) "Feature E"

#### Tree of Bookmarks

(`description.format.tree = "tree"`)

This PR is part of a tree containing 8 PRs:

- `main`

    - [#1](#) "Feature A"

        1. [#2](#) "Feature B"

            1. [#3](#) "Feature C"

                1. [#7](#) "Feature G"

                    - [#8](#) "Feature H"

                2. [#6](#) "Feature F"

            2. **"Feature E" ← this PR**

        2. [#4](#) "Feature D"

#### Complex Graph of Bookmarks

(`description.format.complex = "tree"`)

This PR is part of a complex set of PRs containing 10 PRs:

- `main`

    1. [#9](#) "Feature I"

        1. [#10](#) "Feature J"

            1. [#7](#) "Feature G" (→ [#3](#), [#5](#) also)

                - [#8](#) "Feature H"

            2. **"Feature E" (→ [#2](#) also) ← this PR**

                - [#7](#) "Feature G" (→ [#3](#), [#10](#) also)

                    - [#8](#) "Feature H"

        2. [#6](#) "Feature F" (→ [#3](#) also)

    2. [#1](#) "Feature A"

        1. [#2](#) "Feature B"

            1. **"Feature E" (→ [#10](#) also) ← this PR**

                - [#7](#) "Feature G" (→ [#3](#), [#10](#) also)

                    - [#8](#) "Feature H"

            2. [#3](#) "Feature C"

                1. [#7](#) "Feature G" (→ [#5](#), [#10](#) also)

                    - [#8](#) "Feature H"

                2. [#6](#) "Feature F" (→ [#9](#) also)

            3. [#4](#) "Feature D" (→ [#1](#) also)

        2. [#4](#) "Feature D" (→ [#2](#) also)

## Credits

- [`jj-spr`](https://github.com/LucioFranco/jj-spr) heavily for inspiration & code approaches
- [`jj-stack`](https://github.com/keanemind/jj-stack) heavily for inspiration & code approaches

## FAQs

### Is this vibe-coded slop?

Don't worry, I berated Claude with profanity until things looked good.

### Ok, but really?

Nah. It may have started out as a test to see how well Claude Code was (conclusion: meh not great), but large swathes of the code has been rewritten by hand at this point. AI-generated code is so verbose and inelegant at times, often 2x the size of the hand-written code that uses Rust best practices. I'm not against AI coding, nor think it will replace developers. Be measured, people.

There are a decent amount of tests, but there could always be more.

### Why a new project?

Well primarily, existing tools did not support GitLab (though `jj-vine` now supports GitLab, GitHub, Forgejo, and Azure DevOps). `jj-spr` was too heavy-handed - it imposes a strict "one pull request per commit" workflow. `jj-stack` was in TypeScript (nothing against it, but seems sane for a `jj` tool to also be built in Rust). My current `jj` workflow was also just different enough that those existing tools did not fit my needs.

## Contributing

All contributions extremely welcome! Please feel free to open an issue or pull request. See [CONTRIBUTING.djot](./CONTRIBUTING.djot) for more details.

## License

[MIT License](./LICENSE)
