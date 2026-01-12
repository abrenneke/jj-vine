# Builds and tests the project
default: build lint test-unit

# Checks the project for errors
check:
    cargo check

# Lints the project
lint: fmt clippy check-readmes

# Runs formatting on all files
fmt:
    cargo fmt

# Checks formatting on all files
fmt-check:
    cargo fmt --check

# Lints the project
clippy:
    cargo clippy --tests

# Builds the project
build:
    cargo build

# Runs all tests, including integration tests (see CONTRIBUTING.djot for more details)
test:
    cargo test

# Runs unit tests (excluding integration tests)
test-unit:
    cargo test --features no-gitlab-tests

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
    
    if [[ ! "$(jj bookmark list -r @)" =~ "main" ]]; then
        echo "Error: You are not on the main bookmark. You are on $(jj log -r @ --template 'self.change_id()' --no-graph)"
        exit 1
    fi
    
    if ! jj status | grep -q "The working copy has no changes."; then
        echo "Error: Working copy is not clean. Please commit your changes first."
        exit 1
    fi

    if [ -z "${CODEBERG_TOKEN:-}" ]; then
        echo "Error: CODEBERG_TOKEN environment variable is not set."
        exit 1
    fi
    
    sed -i'' 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
    cargo build --release
    jj commit -m "chore: bump version to {{VERSION}}"
    jj bookmark set main -r @-
    jj tag set v{{VERSION}} -r @-
    jj git push
    cargo publish
    
    http post https://codeberg.org/api/v1/repos/abrenneke/jj-vine/releases \
        -A bearer -a $CODEBERG_TOKEN \
        tag_name=v{{VERSION}} \
        name=v{{VERSION}} \
        body="Release v{{VERSION}}." \
        draft:=true \
        prerelease:=false
        
    echo "Release v{{VERSION}} created successfully."
    echo "Finish release notes at: https://codeberg.org/abrenneke/jj-vine/releases/v{{VERSION}}/edit"