SHELL := /bin/bash
.DEFAULT_GOAL := help
# Release steps mutate the same bundle and must run in order, even with make -j.
.NOTPARALLEL:

APPLE_TEAM_ID ?= PBHS7U4BMU
NOTARY_PROFILE ?= tuck-notary
APPLE_ID ?=
SIGNING_IDENTITY ?= $(shell security find-identity -v -p codesigning | awk '/Developer ID Application: Lyon Cubs, LLC/ {print $$2; exit}')
VERSION := $(shell node -p "require('./package.json').version")
ARCH := $(shell uname -m)
APP := src-tauri/target/release/bundle/macos/Tuck.app
RELEASE_DIR := src-tauri/target/release/bundle/distribution
DMG := $(RELEASE_DIR)/Tuck_$(VERSION)_$(ARCH).dmg
ZIP := $(RELEASE_DIR)/Tuck-notarization.zip
INSTALL_DIR ?= /Applications

.PHONY: help auth notary-check deps native icons signed-app notarize-app dmg notarize-dmg verify install refresh-icons release

help:
	@echo 'make auth APPLE_ID=you@example.com  Store notarization credentials in Keychain (once)'
	@echo 'make release                       Build, sign, notarize, package, verify, install'
	@echo 'make signed-app                    Build a signed app without notarization'
	@echo 'make notarize-app dmg notarize-dmg verify install  Finish an existing signed build'
	@echo 'Overrides: NOTARY_PROFILE, SIGNING_IDENTITY, APPLE_TEAM_ID, INSTALL_DIR'

# notarytool prompts securely for the app-specific password; never put it in make arguments.
auth:
	@test -n "$(APPLE_ID)" || { echo 'Set APPLE_ID to your Apple developer account email.' >&2; exit 1; }
	xcrun notarytool store-credentials "$(NOTARY_PROFILE)" --apple-id "$(APPLE_ID)" --team-id "$(APPLE_TEAM_ID)"

notary-check:
	xcrun notarytool history --keychain-profile "$(NOTARY_PROFILE)" --output-format json >/dev/null

deps:
	npm ci

native:
	npm run native:setup

icons:
	npm exec tauri icon -- assets/icon.png --output src-tauri/icons --ios-color '#003DA5'

signed-app: deps native icons
	@test -n "$(SIGNING_IDENTITY)" || { echo 'No Developer ID Application identity found; set SIGNING_IDENTITY.' >&2; exit 1; }
	@for helper in tuck-audio tuck-calendar tuck-speech; do \
		codesign --force --options runtime --timestamp --sign "$(SIGNING_IDENTITY)" \
			--entitlements src-tauri/Entitlements.plist "src-tauri/binaries/native/$$helper" || exit $$?; \
	done
	APPLE_SIGNING_IDENTITY="$(SIGNING_IDENTITY)" npm run desktop:build -- --bundles app
	codesign --verify --deep --strict --verbose=2 "$(APP)"

notarize-app: notary-check
	codesign --verify --deep --strict --verbose=2 "$(APP)"
	mkdir -p "$(RELEASE_DIR)"
	ditto -c -k --sequesterRsrc --keepParent "$(APP)" "$(ZIP)"
	xcrun notarytool submit "$(ZIP)" --keychain-profile "$(NOTARY_PROFILE)" --wait --timeout 60m
	xcrun stapler staple "$(APP)"
	xcrun stapler validate "$(APP)"

# Avoid Finder AppleScript automation, which can hang unattended builds.
dmg:
	mkdir -p "$(RELEASE_DIR)"
	@set -eu; stage=$$(mktemp -d "$${TMPDIR:-/tmp}/tuck-dmg.XXXXXX"); \
		trap 'rm -rf "$$stage"' EXIT; \
		ditto "$(APP)" "$$stage/Tuck.app"; \
		ln -s /Applications "$$stage/Applications"; \
		hdiutil create -ov -volname Tuck -srcfolder "$$stage" -format UDZO "$(DMG)"
	codesign --force --timestamp --sign "$(SIGNING_IDENTITY)" "$(DMG)"
	codesign --verify --strict --verbose=2 "$(DMG)"

notarize-dmg: notary-check
	xcrun notarytool submit "$(DMG)" --keychain-profile "$(NOTARY_PROFILE)" --wait --timeout 60m
	xcrun stapler staple "$(DMG)"

verify:
	codesign --verify --deep --strict --verbose=2 "$(APP)"
	xcrun stapler validate "$(APP)"
	spctl --assess --type execute --verbose=2 "$(APP)"
	codesign --verify --strict --verbose=2 "$(DMG)"
	xcrun stapler validate "$(DMG)"
	hdiutil verify "$(DMG)"

# Quit Tuck before replacing an existing installation. This target does not force-quit it.
install:
	@if pgrep -x tuck >/dev/null; then \
		echo 'Quit Tuck, then run make install again.' >&2; exit 1; \
	fi
	codesign --verify --deep --strict --verbose=2 "$(APP)"
	ditto "$(APP)" "$(INSTALL_DIR)/Tuck.app"
	codesign --verify --deep --strict --verbose=2 "$(INSTALL_DIR)/Tuck.app"
	$(MAKE) refresh-icons

# Refresh this bundle only; do not clear the global Launch Services database.
refresh-icons:
	touch "$(INSTALL_DIR)/Tuck.app"
	/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$(INSTALL_DIR)/Tuck.app"

release: notary-check deps native signed-app notarize-app dmg notarize-dmg verify install
