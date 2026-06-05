# Ember Trove — developer convenience targets.
# These wrap the same commands CI and the git hooks run, so local and CI behaviour
# match. `cargo` must be on PATH (export PATH="$HOME/.cargo/bin:$PATH").

.PHONY: help hooks-install fmt fmt-check lint lint-ui test check-ui coverage verify

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | \
		awk -F':.*## ' '{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

hooks-install: ## Point git at the version-controlled hooks (scripts/hooks)
	git config core.hooksPath scripts/hooks
	@chmod +x scripts/hooks/* 2>/dev/null || true
	@echo "git hooks installed (core.hooksPath=scripts/hooks)."

fmt: ## Format the whole workspace
	cargo fmt --all

fmt-check: ## Check formatting (CI gate)
	cargo fmt --all --check

lint: ## Clippy on api + common, warnings = errors
	cargo clippy --workspace --exclude ui -- -D warnings

lint-ui: ## Clippy on ui for the wasm32 target
	cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings

test: ## Run the host test suite (excludes the WASM ui crate)
	cargo test --workspace --exclude ui

check-ui: ## Type-check ui for the wasm32 target
	cargo check -p ui --target wasm32-unknown-unknown

coverage: ## Line coverage over api + common (needs cargo-llvm-cov)
	cargo llvm-cov --workspace --exclude ui --summary-only

verify: ## Full local verification suite (matches scripts/verify.sh)
	./scripts/verify.sh
