# book-converter — build and development tasks
# Stack: Tauri v2 (Rust core) + React/TS frontend

TAURI_DIR := src-tauri

.DEFAULT_GOAL := help

# List available targets
help:
	@echo "book-converter — available commands:"
	@echo "  make install     install dependencies (npm + tauri-cli)"
	@echo "  make deps-linux  Tauri system packages for Linux (needs sudo)"
	@echo "  make dev         run the app in dev mode"
	@echo "  make build       build a release bundle"
	@echo "  make run         build and run the core (no frontend)"
	@echo "  make check       cargo check + tsc (no build)"
	@echo "  make fmt         format (cargo fmt)"
	@echo "  make lint        cargo clippy"
	@echo "  make test        cargo test"
	@echo "  make clean       clean build artifacts"

# --- Dependencies ---

# Install frontend deps and tauri-cli
install:
	@echo "Installing frontend deps..."
	@npm install
	@echo "Installing tauri-cli..."
	@cargo install tauri-cli --locked

# Tauri system libraries for Linux (Debian/Ubuntu)
deps-linux:
	@echo "Installing Tauri system libs (sudo)..."
	@sudo apt update
	@sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
		libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

# --- Development ---

# Dev mode: Vite + Tauri with hot-reload
dev:
	@npm run tauri dev

# Release bundle
build:
	@npm run tauri build

# Build and run the Rust core only (handy for CLI checks of the core)
run:
	@cd $(TAURI_DIR) && cargo run

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

.PHONY: help install deps-linux dev build run check fmt lint test clean
