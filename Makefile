PREFIX ?= /usr/local
LIBEXECDIR ?= $(PREFIX)/libexec
PYTHON ?= python3
CARGO ?= cargo

.PHONY: all themes server check test install clean
all: themes server
themes:
	npm run build
server:
	$(MAKE) -C server
check test: all
	$(MAKE) -C server test-platform
	$(CARGO) test --manifest-path rust/Cargo.toml --package xtask --locked --offline
	$(PYTHON) tools/desktop/check.py
install:
	$(CARGO) xtask install --prefix '$(PREFIX)' --libexecdir '$(LIBEXECDIR)' --destdir '$(DESTDIR)'
clean:
	$(MAKE) -C server clean
