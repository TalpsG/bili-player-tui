#!/usr/bin/env bash

# 强制退出码为 0，防止 set -u 误伤
: ${LIBRARY_PATH:=}
: ${PKG_CONFIG_PATH:=}
: ${RUSTFLAGS:=}

# Homebrew 在 Apple Silicon 上的标准路径
BREW_PREFIX="/opt/homebrew"
MPV_LIB="$BREW_PREFIX/lib"

# 注入编译和运行时环境
export LIBRARY_PATH="$MPV_LIB:$LIBRARY_PATH"
# libmpv has an absolute Homebrew install name. Do not inject
# DYLD_LIBRARY_PATH: it also overrides macOS ImageIO dependencies and can
# crash mpv_create() while loading the media-controls icon.
export PKG_CONFIG_PATH="$MPV_LIB/pkgconfig:$PKG_CONFIG_PATH"
export RUSTFLAGS="-L $MPV_LIB $RUSTFLAGS"

# 执行 cargo，并将所有参数原样传递
exec cargo "$@"
