#!/bin/sh
# The system libraries GPUI's Linux backend (X11, Wayland, Vulkan, fonts) builds against.
set -eu
sudo apt-get update -q
sudo apt-get install -y -q --no-install-recommends \
    pkg-config libssl-dev libzstd-dev \
    libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libvulkan-dev libfontconfig1-dev libfreetype-dev
