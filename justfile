# Builds and tests the project
default: build lint test-unit

# Checks the project for errors
check *args:
    cargo check {{args}}

# Lints the project
lint: fmt clippy check-readmes

# Runs formatting on all files
fmt *args:
    cargo fmt {{args}}

# Checks formatting on all files
fmt-check *args:
    cargo fmt --check {{args}}

# Lints the project
clippy *args:
    cargo clippy --tests {{args}}

# Builds the project
build *args:
    cargo build {{args}}

# Runs all tests, including integration tests (see CONTRIBUTING.djot for more details)
test *args:
    cargo test {{args}}

# Runs unit tests (excluding integration tests)
test-unit *args:
    cargo test --features no-e2e-tests {{args}}

# Ensures that the .djot files are synced to the .md files.
check-readmes:
    cmp -s README.djot README.md
    cmp -s CONTRIBUTING.djot CONTRIBUTING.md

# Syncs the .djot files to the .md files. The .djot files are the source of truth.
sync-readmes:
    cp README.djot README.md
    cp CONTRIBUTING.djot CONTRIBUTING.md

# Release a new version. Usage: just release 0.2.0
[private]
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail

    if [[ ! "{{VERSION}}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "Error: Version must be in format X.Y.Z (e.g., 0.2.0)"
        exit 1
    fi

    if [ -z "$CODEBERG_TOKEN" ]; then
        echo "Error: CODEBERG_TOKEN environment variable is not set."
        exit 1
    fi

    if [ -z "$GITHUB_TOKEN" ]; then
        echo "Error: GITHUB_TOKEN environment variable is not set."
        exit 1
    fi

    jj new main

    sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml

    cargo build --release

    jj commit -m "chore: bump version to {{VERSION}}"
    jj bookmark set main -r @-
    jj tag set v{{VERSION}} -r @-
    jj git push

    cargo publish

    echo "Waiting 10 seconds for Codeberg to sync to GitHub..."
    sleep 10

    http post https://api.github.com/repos/abrenneke/jj-vine/actions/workflows/ci.yml/dispatches \
        -A bearer -a "$GITHUB_TOKEN" \
        ref=main \
        inputs:='{"tag_name":"v{{VERSION}}"}'

    http post https://codeberg.org/api/v1/repos/abrenneke/jj-vine/releases \
        -A bearer -a "$CODEBERG_TOKEN" \
        --print h \
        tag_name=v{{VERSION}} \
        name=v{{VERSION}} \
        body="Release v{{VERSION}}." \
        draft:=true \
        prerelease:=false

    echo "Release v{{VERSION}} created successfully."
    echo "Finish release notes at: https://codeberg.org/abrenneke/jj-vine/releases/edit/v{{VERSION}}"

# Starts a forgejo server using docker compose. Codeberg has low rate limits, so for integration tests we need to run our own instance.
start-forgejo:
    docker compose -f forgejo.docker-compose.yml up -d

stop-forgejo:
    docker compose -f forgejo.docker-compose.yml down