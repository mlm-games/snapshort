# Snapshort Editor - Development Tasks

# Default recipe
default: check

# Check all crates
check:
    cargo check --workspace

# Build all crates
build:
    cargo build --workspace

# Build release
build-release:
    cargo build --workspace --release

# Run desktop app
run:
    cargo run -p snapshort-desktop

# Run CLI
cli *ARGS:
    cargo run -p snapshort-cli -- {{ARGS}}

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Format code
fmt:
    cargo fmt --all

# Lint code
lint:
    cargo clippy --workspace -- -D warnings

# Clean build artifacts
clean:
    cargo clean

# Generate docs
docs:
    cargo doc --workspace --no-deps --open

# Watch and rebuild on changes
watch:
    cargo watch -x 'check --workspace'

# Create new release
release VERSION:
    @echo "Creating release {{VERSION}}"
    git tag -a "v{{VERSION}}" -m "Release {{VERSION}}"
    cargo build --workspace --release
