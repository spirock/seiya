.PHONY: test test-python test-smoke-harness test-rust test-image-ui serve deb mac dmg deb-python mac-python dmg-python

PYTHON ?= python3
NODE ?= node
RUST_BIN_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),$(CURDIR)/target)/release
RUST_ARCH ?= $(shell uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')

test: test-python test-smoke-harness test-rust test-image-ui

test-python:
	cd swarm-app && $(PYTHON) -m unittest discover -s tests -v

# Offline evidence/plan checks only. Real inference is always manual/opt-in.
test-smoke-harness:
	$(PYTHON) -m unittest discover -s local-tools-tests -p 'test_seiya_rust_smoke.py' -v
	$(PYTHON) -m unittest discover -s local-tools-tests -p 'test_memory_baseline_plan.py' -v
	$(PYTHON) -m unittest discover -s local-tools-tests -p 'test_r1_static_audit.py' -v
	$(PYTHON) -m unittest discover -s local-tools-tests -p 'test_ollama_private.py' -v

test-rust:
	cargo test --workspace --locked

test-image-ui:
	$(NODE) swarm-app/tests/desktop_setup_ui.cjs
	$(NODE) swarm-app/tests/images_ui.cjs
	$(NODE) swarm-app/tests/history_ui.cjs
	$(NODE) swarm-app/tests/diagnostics_ui.cjs
	$(NODE) swarm-app/tests/chat_images_ui.cjs
	$(NODE) swarm-app/tests/desktop_setup_feedback.cjs

serve:
	cd swarm-app && $(PYTHON) -m seiya serve --open

deb:
	$(PYTHON) swarm-app/packaging/build.py deb --bin-dir "$(RUST_BIN_DIR)" --arch "$(RUST_ARCH)" --libc-min "$(LIBC_MIN)"

mac:
	$(PYTHON) swarm-app/packaging/build.py mac --bin-dir "$(RUST_BIN_DIR)" --arch "$(RUST_ARCH)"

dmg:
	$(PYTHON) swarm-app/packaging/build.py dmg --bin-dir "$(RUST_BIN_DIR)" --arch "$(RUST_ARCH)"

deb-python:
	$(PYTHON) swarm-app/packaging/build.py deb --implementation python

mac-python:
	$(PYTHON) swarm-app/packaging/build.py mac --implementation python

dmg-python:
	$(PYTHON) swarm-app/packaging/build.py dmg --implementation python
