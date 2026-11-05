# Tauri + Leptos frontend

This folder contains the complete desktop app: the Leptos UI, Tauri Rust host, icons, and frontend build configuration. The signaling service is maintained separately in the sibling `../backend/` directory.

## Development

Run these commands from `frontend/`:

```sh
trunk serve
cargo tauri dev
```

For a deployed signaling service, set its Terraform `signaling_ws_url` output at build time:

```sh
SIGNALING_WS_URL="wss://your-cloud-run-service-url" cargo tauri build
```

Leave the variable unset for local development; the app defaults to `ws://127.0.0.1:9001`. See [the backend deployment guide](../backend/README.md) for Cloud Run setup.

## Recommended IDE setup

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://rust-analyzer.github.io/).
