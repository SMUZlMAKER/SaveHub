# SaveHub

SaveHop is a system tray app that switches game save profiles across all your games at once and syncs them between PCs via your own GitHub repository. Built for games without cloud saves.

## Frontend ↔ backend transport

The Windows application uses Tauri IPC (`invoke`) instead of a localhost HTTP
server. This is the intended boundary for a single-process desktop application:
it avoids port management, serialization duplicated in an HTTP layer, and an
unnecessary local network attack surface. The frontend calls only
`frontend/src/lib/api.ts`; Rust commands are registered in
`src/app/app.rs`.

The product requirements can still be described as resource operations:

| Product operation | Tauri command |
| --- | --- |
| `GET /me` | `current_profile` |
| `GET /games/{game}/status` | `get_game_status` |
| `GET /games` | `list_games` |
| `PUT /saves/{game}` | `add_game` / `set_game_save_path` |
| `GET /saves/{game}` | `list_saves` |
| `POST /saves/{game}/choose` | `choose_save` |
| `GET /saves/{game}/conflict` | `get_save_conflict` |
| `POST /saves/{game}/conflict` | `resolve_save_conflict` |
| `GET /profiles` | `list_profiles` |
| `POST /profile` | `create_profile` |
| `POST /profiles/set?profile_id=...` | `switch_profile` |

Commands are intentionally thin. File operations, process detection and git
integration belong in adapters; orchestration belongs in services/workflows.
If a future web, CLI or remote client is required, the same service layer can
be exposed through HTTP without coupling the desktop frontend to HTTP today.

## Contributing guide (russian)

See [CONTRIBUTING.md](CONTRIBUTING.md)
