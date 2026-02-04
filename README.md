# VitaminK

KDE system tray app for intelligent Sunshine game streaming display management.

## What is VitaminK?

VitaminK automatically manages displays for Sunshine game streaming on KDE Plasma:
- Detects when your monitor goes to sleep and enables a dummy plug for streaming
- Automatically switches to the correct display and resolution when you connect
- Restores your desktop when you're done streaming

Built in Rust with a simple GUI to configure everything.

## Status

🚧 **Early Development** - Currently building Phase 1: Configuration wizard

## Requirements

- KDE Plasma (Wayland)
- Sunshine game streaming server
- HDMI dummy plug (4K HDR recommended)
- Rust 1.93+

## Development

```bash
cargo build
cargo run
```

## License

GPL-3.0
