# Snapshort Video Editor

A professional video editor built with Rust, featuring:

- 🎬 GPU-accelerated timeline editing
- 🎥 FFmpeg-based media decoding
- 🖥️ Native desktop UI with egui/eframe
- 🔄 Full undo/redo support
- 📁 SQLite-based project storage
- 🎨 WGPU rendering pipeline

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       Desktop App                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │   Views     │  │    State    │  │       Theme         │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
┌─────────────────────────────────────────────────────────────┐
│                       Use Cases                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Timeline   │  │    Asset    │  │      Project        │ │
│  │   Service   │  │   Service   │  │      Service        │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
┌─────────────────────────────────────────────────────────────┐
│                        Domain                                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Timeline   │  │    Asset    │  │        Clip         │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
┌─────────────────────────────────────────────────────────────┐
│                    Infrastructure                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Database   │  │    Media    │  │       Render        │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Build all crates
cargo build --workspace

# Run the desktop app
cargo run -p snapshort-desktop

# Run tests
cargo test --workspace

# Using just (if installed)
just run
just test
```

## Crates

| Crate | Description |
|-------|-------------|
| `domain` | Pure business logic, entities, value objects |
| `usecases` | Application services, commands, events |
| `infra-db` | SQLite persistence layer |
| `infra-media` | FFmpeg decoding, analysis |
| `infra-render` | WGPU rendering pipeline |
| `infra-ai` | AI integrations (future) |
| `ui-core` | Shared UI components |
| `desktop` | Main desktop application |
| `cli` | Command-line interface |

## License

MIT
