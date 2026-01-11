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