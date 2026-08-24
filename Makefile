# Plain install targets. Wander has no generated sources and no build-time
# configuration, so cargo is the build system and this only puts the results
# where a desktop expects to find them.

PREFIX  ?= /usr/local
DESTDIR ?=
APP_ID  := io.github.muhnschein.Wander

BIN     := $(DESTDIR)$(PREFIX)/bin
APPS    := $(DESTDIR)$(PREFIX)/share/applications
META    := $(DESTDIR)$(PREFIX)/share/metainfo
ICONS   := $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps

.PHONY: all build install uninstall check check-data clean

all: build

build:
	cargo build --release

install: build
	install -Dm755 target/release/wander $(BIN)/wander
	install -Dm644 data/$(APP_ID).desktop $(APPS)/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).metainfo.xml $(META)/$(APP_ID).metainfo.xml
	install -Dm644 data/icons/hicolor/scalable/apps/$(APP_ID).svg $(ICONS)/$(APP_ID).svg
	@echo
	@echo "Installed. If this is a real prefix rather than a staging directory,"
	@echo "refresh the desktop caches so the launcher and icon appear:"
	@echo "  update-desktop-database $(PREFIX)/share/applications"
	@echo "  gtk-update-icon-cache -f -t $(PREFIX)/share/icons/hicolor"

uninstall:
	rm -f $(BIN)/wander
	rm -f $(APPS)/$(APP_ID).desktop
	rm -f $(META)/$(APP_ID).metainfo.xml
	rm -f $(ICONS)/$(APP_ID).svg

# Everything CI runs, so it can be run the same way locally.
check: check-data
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

check-data:
	desktop-file-validate data/$(APP_ID).desktop
	appstreamcli validate --no-net data/$(APP_ID).metainfo.xml

clean:
	cargo clean
