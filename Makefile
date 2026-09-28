.PHONY: help build build-server build-extension test test-wasm check fmt clippy clean

.DEFAULT_GOAL := help

help: ## Show this help (default when run with no target)
	@awk 'BEGIN {FS = ":.*##"} /^[a-zA-Z_-]+:.*##/ {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: build-server build-extension ## Release build of both: server binary + extension .wasm

build-server: ## Release build of the jbehave-language-server binary
	cargo build --release -p jbehave-language-server

build-extension: ## Release build of the extension for wasm32-wasip2
	cargo build --release -p zed-jbehave --target wasm32-wasip2

test: ## Run the language server's test suite (unit + integration) and the grammar crate's tests
	cargo test -p jbehave-language-server
	cargo test -p tree-sitter-jbehave-sys

WASI_SDK_PATH ?= /opt/wasi-sdk-25.0-x86_64-linux
test-wasm: ## Run the grammar crate's tests under wasm32-wasip2 (same runtime Zed uses)
	env WASI_SDK_PATH=$(WASI_SDK_PATH) \
	    CC_wasm32-wasip2=$(WASI_SDK_PATH)/bin/clang \
	    WASI_SYSROOT=$(WASI_SDK_PATH)/share/wasi-sysroot \
	    cargo test -p tree-sitter-jbehave-sys --target wasm32-wasip2

check: ## cargo check the workspace (extension + server + sys, fast, no codegen)
	cargo check -p zed-jbehave --target wasm32-wasip2
	cargo check -p jbehave-language-server

fmt: ## Format all crates with cargo fmt
	cargo fmt --all

clippy: ## Lint the workspace with clippy, denying warnings
	cargo clippy -p zed-jbehave --target wasm32-wasip2 -- -D warnings
	cargo clippy -p jbehave-language-server -- -D warnings
	cargo clippy -p tree-sitter-jbehave-sys -- -D warnings

clean: ## Remove all build artifacts
	cargo clean
