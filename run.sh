#!/bin/bash
# Document Processor - Launch script

cd "$(dirname "$0")"

# Check if dependencies are installed
check_deps() {
    if ! command -v node &> /dev/null; then
        echo "Node.js is required. Install with: sudo apt install nodejs npm"
        exit 1
    fi

    if ! command -v cargo &> /dev/null; then
        echo "Rust is required. Install with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
        echo "WebKit2GTK dev package required. Install with:"
        echo "sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev"
        exit 1
    fi
}

# Install npm dependencies if needed
install_deps() {
    if [ ! -d "node_modules" ]; then
        echo "Installing npm dependencies..."
        npm install
    fi
}

# Source cargo env
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

case "${1:-dev}" in
    dev)
        check_deps
        install_deps
        echo "Starting Document Processor in development mode..."
        npm run tauri dev
        ;;
    build)
        check_deps
        install_deps
        echo "Building Document Processor..."
        npm run tauri build
        echo "Build complete! Check src-tauri/target/release/bundle/"
        ;;
    install-deps)
        echo "Installing system dependencies..."
        sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev
        npm install
        echo "Dependencies installed!"
        ;;
    preflight)
        exec "$(dirname "$0")/bin/preflight"
        ;;
    *)
        echo "Usage: ./run.sh [dev|build|install-deps|preflight]"
        echo "  dev          - Run in development mode (default)"
        echo "  build        - Build release version"
        echo "  install-deps - Install system and npm dependencies"
        echo "  preflight    - Check the host has everything to build (read-only)"
        ;;
esac
