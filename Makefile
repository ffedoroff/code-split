.PHONY: all build test clippy lint-md lint check fmt clean

all: build test lint

build:
	cargo build --workspace

test:
	cargo test --workspace

clippy:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all

lint-md:
	lychee --offline --no-progress 'docs/**/*.md' 'principles/**/*.md' 'AGENTS.md' 'CLAUDE.md'
	markdownlint-cli2

lint: clippy lint-md

check: build test clippy lint-md

clean:
	cargo clean
