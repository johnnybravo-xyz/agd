ifeq ($(shell uname -s),Darwin)
APPLE_CLANG := $(shell xcrun --find clang)
export CC := $(APPLE_CLANG)
export CXX := $(shell xcrun --find clang++)
export SDKROOT := $(shell xcrun --show-sdk-path)
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER := $(APPLE_CLANG)
export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER := $(APPLE_CLANG)
endif

.PHONY: build

build:
	cargo build --workspace --all-targets
