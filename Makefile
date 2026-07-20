# book-converter — build and development tasks
# Stack: Tauri v2 (Rust core) + React/TS frontend

TAURI_DIR := src-tauri
BIN := $(TAURI_DIR)/target/release/book-converter
# Use the local Tauri CLI via npx: `npm run tauri dev` does NOT forward the
# subcommand under npm 11.
TAURI := npx tauri

.DEFAULT_GOAL := help

# List available targets
help:
	@echo "book-converter — available commands:"
	@echo "  make install     install frontend + CLI deps (npm)"
	@echo "  make deps-linux  Tauri system packages for Linux (needs sudo)"
	@echo "  make dev         run the app with hot-reload (Vite + window)"
	@echo "  make binary      build a standalone release binary (no installer)"
	@echo "  make bundle      build installers (.deb / .rpm / .AppImage)"
	@echo "  make run         build the release binary and launch it"
	@echo "  make check       cargo check + tsc (no build)"
	@echo "  make fmt         format (cargo fmt)"
	@echo "  make lint        cargo clippy"
	@echo "  make test        cargo test"
	@echo "  make clean       clean build artifacts"

# --- Dependencies ---

# Install frontend + Tauri CLI (devDependency) via npm
install:
	@echo "Installing deps..."
	@npm install

# Tauri system libraries for Linux (Debian/Ubuntu)
deps-linux:
	@echo "Installing Tauri system libs (sudo)..."
	@sudo apt update
	@sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
		libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

# --- Development ---

# Dev mode: Vite dev server + Tauri window with hot-reload
dev:
	@$(TAURI) dev

# Standalone release binary with the frontend embedded (runs without Vite)
binary:
	@$(TAURI) build --no-bundle
	@echo "Binary: $(BIN)"

# Full installers (.deb / .rpm / .AppImage on Linux)
bundle:
	@$(TAURI) build

# Build the release binary and run it
run: binary
	@./$(BIN)

# --- Quality ---

# Fast type check without a build
check:
	@echo "cargo check..."
	@cd $(TAURI_DIR) && cargo check
	@echo "tsc..."
	@npx tsc --noEmit

# Format Rust
fmt:
	@cd $(TAURI_DIR) && cargo fmt

# Lint Rust
lint:
	@cd $(TAURI_DIR) && cargo clippy --all-targets -- -D warnings

# Rust tests
test:
	@cd $(TAURI_DIR) && cargo test

# --- Cleanup ---

clean:
	@echo "Cleaning build artifacts..."
	@cd $(TAURI_DIR) && cargo clean
	@rm -rf dist node_modules

.PHONY: help install deps-linux dev binary bundle run check fmt lint test clean
