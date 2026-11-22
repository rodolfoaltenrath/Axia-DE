PREFIX ?= /usr/local
LIBEXECDIR ?= $(PREFIX)/libexec
PYTHON ?= python3
CARGO ?= cargo

.PHONY: all themes server check test install clean
all: themes server
themes:
	$(CARGO) xtask build-theme
server:
	$(MAKE) -C server
check test: all
	$(MAKE) -C server test-platform
	$(CARGO) test --manifest-path rust/Cargo.toml --package xtask --locked --offline
	$(CARGO) test --manifest-path rust/Cargo.toml --package xtask --locked --offline --test shell_surfaces_runtime -- --ignored --nocapture
	$(PYTHON) tools/desktop/check.py
install:
	$(CARGO) xtask install --prefix '$(PREFIX)' --libexecdir '$(LIBEXECDIR)' --destdir '$(DESTDIR)'
clean:
	$(MAKE) -C server clean
