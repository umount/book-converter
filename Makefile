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
	@echo "  make install       install frontend + CLI deps (npm)"
	@echo "  make deps-linux    Tauri system packages for Linux (needs sudo)"
	@echo "  make fetch-pdfium  download the pdfium PDF engine for this platform"
	@echo "  make dev         run the app with hot-reload (Vite + window)"
	@echo "  make binary      build a standalone release binary (no installer)"
	@echo "  make bundle      build installers (.deb / .rpm / .AppImage)"
	@echo "  make run         build the release binary and launch it"
	@echo "  make check       cargo check + tsc (no build)"
	@echo "  make fmt         format (cargo fmt)"
	@echo "  make lint        cargo clippy"
	@echo "  make test        cargo test"
	@echo "  make version V=x.y.z  set the release version everywhere"
	@echo "  make show-version     show declared version + build stamp"
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

# --- PDF engine (pdfium) ---

# Download the prebuilt pdfium library for THIS platform into src-tauri/pdfium/.
# Cross-platform PDF text extraction relies on it (bundled as a Tauri resource).
# Run once before `make binary`/`bundle`; re-run when building on a new platform.
fetch-pdfium:
	@bash scripts/fetch-pdfium.sh

# --- Development ---

# Dev mode: Vite dev server + Tauri window with hot-reload
dev: fetch-pdfium
	@$(TAURI) dev

# Standalone release binary with the frontend embedded (runs without Vite)
binary: fetch-pdfium
	@$(TAURI) build --no-bundle
	@mkdir -p $(TAURI_DIR)/target/release/pdfium
	@cp -f $(TAURI_DIR)/pdfium/* $(TAURI_DIR)/target/release/pdfium/ 2>/dev/null || true
	@rm -rf $(TAURI_DIR)/target/release/tts-runtime
	@cp -R $(TAURI_DIR)/tts-runtime $(TAURI_DIR)/target/release/tts-runtime
	@echo "Binary: $(BIN)"

# Full installers (.deb / .rpm / .AppImage on Linux)
bundle: fetch-pdfium
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

# --- Version ---

# Set the release version in the three places that declare it, so the About
# dialog, the bundle and npm never disagree: make version V=0.2.0
version:
	@test -n "$(V)" || { echo "usage: make version V=x.y.z"; exit 1; }
	@echo "$(V)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "version must be x.y.z"; exit 1; }
	@sed -i 's/^version = ".*"/version = "$(V)"/' $(TAURI_DIR)/Cargo.toml
	@sed -i 's/"version": "[^"]*"/"version": "$(V)"/' $(TAURI_DIR)/tauri.conf.json
	@npm pkg set version=$(V) >/dev/null
	@cd $(TAURI_DIR) && cargo update -p book-converter --quiet 2>/dev/null || true
	@echo "version set to $(V) (Cargo.toml, tauri.conf.json, package.json)"

# Show the version currently declared, and the build stamp About would report
show-version:
	@printf 'cargo:      %s\n' "$$(sed -n 's/^version = "\(.*\)"/\1/p' $(TAURI_DIR)/Cargo.toml | head -1)"
	@printf 'tauri.conf: %s\n' "$$(sed -n 's/.*"version": "\([^"]*\)".*/\1/p' $(TAURI_DIR)/tauri.conf.json | head -1)"
	@printf 'package:    %s\n' "$$(node -p "require('./package.json').version")"
	@printf 'commit:     %s (%s)\n' "$$(git rev-parse --short=9 HEAD)" "$$(git log -1 --format=%cs)"

# --- Cleanup ---

clean:
	@echo "Cleaning build artifacts..."
	@cd $(TAURI_DIR) && cargo clean
	@rm -rf dist node_modules

.PHONY: help install deps-linux dev binary bundle run check fmt lint test version show-version clean
