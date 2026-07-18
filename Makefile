RUST_CRATES := \
	packages/architect-c4-domain \
	packages/architect-c4-revision \
	packages/architect-c4-session \
	packages/architect-c4-model \
	packages/architect-c4-git \
	packages/architect-c4-adr \
	packages/architect-c4-validate \
	packages/architect-c4-render

.PHONY: develop test cov-py cov-rust cov fmt lint ci-local

develop:
	uv sync --extra dev
	uv run maturin develop

test:
	cargo test --workspace --exclude architect-c4-app
	uv run maturin develop
	uv run pytest -q

cov-rust:
	chmod +x scripts/*.sh
	./scripts/rust-coverage.sh

cov-py:
	chmod +x scripts/*.sh
	./scripts/python-coverage.sh

cov: cov-rust cov-py

fmt:
	cargo fmt --all
	uv run ruff format python tests

lint:
	@set -e; for d in $(RUST_CRATES); do (cd $$d && cargo clippy --all-targets -- -D warnings); done
	uv run ruff check python tests

ci-local: lint test cov
