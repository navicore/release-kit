# release-kit justfile
# Run `just` to see available commands

# Default command: show available recipes
default:
    @just --list

# Build all crates in the workspace
build:
    cargo build

# Build with optimizations
build-release:
    cargo build --release

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy lints
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Check code without building
check:
    cargo check --workspace --all-targets --all-features

# Format all code
fmt:
    cargo fmt --all

# Check if code is formatted
fmt-check:
    cargo fmt --all -- --check

# Run all pre-commit checks (format, lint, test)
pre-commit: fmt lint test

# Run all CI checks (what GitHub Actions runs)
ci: fmt-check lint test build-release

# Clean build artifacts
clean:
    cargo clean

# Update dependencies
update:
    cargo update

# Build the Cloudflare Worker (WASM target)
build-worker:
    cd crates/worker-template && cargo build --target wasm32-unknown-unknown --release

# Run the CLI with args (e.g., `just run -- --help`)
run *ARGS:
    cargo run -- {{ARGS}}

# Validate the example album
validate-example:
    cargo run -- validate examples/test-album/

# Preview the example album (not yet implemented)
preview-example:
    cargo run -- preview examples/test-album/

# Install the CLI locally
install:
    cargo install --path crates/cli

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

# Check for outdated dependencies
outdated:
    cargo outdated

# Audit dependencies for security vulnerabilities
audit:
    cargo audit

# Generate shell completions (bash, zsh, fish, powershell, elvish)
completions SHELL:
    cargo run -- completions {{SHELL}}

# Install bash completions to user directory
install-completions-bash:
    mkdir -p ~/.local/share/bash-completion/completions
    cargo run -- completions bash > ~/.local/share/bash-completion/completions/release-kit
    @echo "Bash completions installed. Restart your shell or run: source ~/.local/share/bash-completion/completions/release-kit"

# Install zsh completions to user directory
install-completions-zsh:
    mkdir -p ~/.zsh/completions
    cargo run -- completions zsh > ~/.zsh/completions/_release-kit
    @echo "Zsh completions installed. Add to ~/.zshrc: fpath=(~/.zsh/completions \$fpath)"
    @echo "Then restart your shell or run: autoload -U compinit && compinit"

# Install fish completions to user directory
install-completions-fish:
    mkdir -p ~/.config/fish/completions
    cargo run -- completions fish > ~/.config/fish/completions/release-kit.fish
    @echo "Fish completions installed. Restart your shell or run: source ~/.config/fish/completions/release-kit.fish"
