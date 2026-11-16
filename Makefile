# chap-devops - build & dev tasks for the chap-devops crate.
CARGO  ?= cargo
ARCHS  := aarch64-apple-darwin x86_64-apple-darwin
BIN    := bin/chap-devops
PREFIX ?= $(HOME)/.local
ARGS   ?=
.DEFAULT_GOAL := help
.PHONY: help check lint test build e2e release run install vendor docs docs-reference docs-serve slides release-pr release-tag clean
help: ## Show this help
	@echo "chap-devops - make targets:"
	@echo ""
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / { printf "  \033[1m%-16s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
check: ## Report formatting and clippy issues, fixing nothing
	$(CARGO) fmt --all --check
	$(CARGO) clippy --all-targets -- -D warnings
lint: ## Fix what check reports: clippy --fix, then format (writes files)
	$(CARGO) clippy --fix --allow-dirty --allow-staged --all-targets
	$(CARGO) fmt --all
test: ## Run the test suite
	$(CARGO) test
build: ## Fast host-arch compile check
	$(CARGO) build
release: ## Build bin/chap-devops: universal (arm64+x86_64) on macOS, host binary on Linux
	@mkdir -p $(dir $(BIN))
	@if [ "$$(uname -s)" = "Darwin" ]; then \
		if command -v rustup >/dev/null 2>&1; then missing=""; for arch in $(ARCHS); do rustup target list --installed | grep -qx "$$arch" || missing="$$missing $$arch"; done; if [ -n "$$missing" ]; then echo "error: missing Rust target(s):$$missing"; echo "       rustup target add$$missing"; exit 1; fi; fi; \
		for arch in $(ARCHS); do echo "==> building $$arch"; $(CARGO) build --release --target $$arch || exit 1; done; \
		lipo -create -output $(BIN) $(foreach arch,$(ARCHS),target/$(arch)/release/chap-devops); \
		lipo -info $(BIN); \
	else \
		echo "==> building host binary"; \
		$(CARGO) build --release || exit 1; \
		cp target/release/chap-devops $(BIN); \
		echo "built $(BIN)"; \
	fi
run: release ## Build the release binary, then run it (ARGS="...")
	$(BIN) $(ARGS)
install: release ## Install bin/chap-devops and its short form cdx into PREFIX/bin (PREFIX defaults to ~/.local)
	@mkdir -p $(PREFIX)/bin
	install -m 0755 $(BIN) $(PREFIX)/bin/chap-devops
	@if [ -e $(PREFIX)/bin/cdx ] && [ "$$(readlink $(PREFIX)/bin/cdx)" != "chap-devops" ]; then echo "installed $(PREFIX)/bin/chap-devops; $(PREFIX)/bin/cdx is another program, so it was left alone"; else ln -sf chap-devops $(PREFIX)/bin/cdx && echo "installed $(PREFIX)/bin/chap-devops, and $(PREFIX)/bin/cdx as its short form"; fi
vendor: ## Refresh the embedded marketplace snapshot in vendor/marketplace/
	scripts/vendor-marketplace.sh
e2e: build ## Run OCS, chap-core, DHIS2 and the models for real (TAGS="latest master")
	scripts/e2e.sh $(if $(TAGS),--tags "$(TAGS)",)
docs-reference: ## Regenerate docs/reference.md from the CLI help texts
	$(CARGO) run -q -- docs-markdown > docs/reference.md
docs: docs-reference ## Build the mdbook documentation into site/
	mdbook build
MARP   ?= bunx @marp-team/marp-cli@4.5.1
slides: ## Build the slide decks into site/slides/: HTML, 16:9 PDF and the index page
	$(MARP) --theme-set slides/theme/chap-devops.css --input-dir slides/decks --output site/slides --html
	$(MARP) --theme-set slides/theme/chap-devops.css --input-dir slides/decks --output site/slides --pdf
	$(MARP) --theme-set slides/theme/chap-devops.css --input-dir slides/decks --output site/slides --image png
	perl -0pi -e 's{<head>(?!<link rel="icon")}{<head><link rel="icon" href="favicon.svg"><link rel="shortcut icon" href="favicon.png">}' site/slides/*.html
	cp slides/index.html site/slides/index.html
	@# The book's favicon, under a name without mdbook's hash, on every page.
	@for ext in svg png; do f=$$(ls site/favicon-*.$$ext 2>/dev/null | head -1); \
		[ -n "$$f" ] && cp "$$f" site/slides/favicon.$$ext || echo "no site/favicon-*.$$ext; run make docs first for the favicon"; done

docs-serve: ## Serve the documentation at localhost:3000 and open a browser
	mdbook serve --open
release-pr: ## Bump the version, run the checks, and open the release pull request (VERSION=x.y.z)
	@test -n "$(VERSION)" || { echo "usage: make release-pr VERSION=x.y.z"; exit 2; }
	CARGO="$(CARGO)" MAKE="$(MAKE)" scripts/release.sh $(VERSION) --pr
release-tag: ## After the release pull request is merged: tag it and start the release (VERSION=x.y.z)
	@test -n "$(VERSION)" || { echo "usage: make release-tag VERSION=x.y.z"; exit 2; }
	scripts/release.sh $(VERSION) --tag
clean: ## Remove target/ and bin/
	$(CARGO) clean
	@rm -rf $(dir $(BIN))
