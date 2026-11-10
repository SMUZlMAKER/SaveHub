# Git из Rust

Используем [git2](https://docs.rs/git2/): Rust-обёртку над libgit2 с поддержкой commit, checkout, clone, fetch и push без установленного Git CLI, для сборки требуется компилятор C / C++.

Альтернативы: [gix](https://github.com/GitoxideLabs/gitoxide) — реализация на Rust; [git-spawn](https://docs.rs/git-spawn/) — асинхронная обёртка над установленным Git CLI.

Запуск из корня проекта:

```bash
cargo run --locked --manifest-path pocs/git/Cargo.toml
```

PoC создаёт один репозиторий в `pocs/git/demo/`: коммитит `example.txt` со значением `version=1`, создаёт ветку `example-branch` и коммитит в ней `version=2`. Затем переключает ветки и проверяет содержимое файла и соблюдение `.gitignore`. Перед коммитами выводит список изменений через `Repository::statuses` (аналог `git status`): новые и изменённые файлы без игнорируемых. В конце показывает чистую рабочую копию. При успехе печатает `OK` и оставляет активной ветку `main`.

Папка `demo/` остаётся после запуска и исключена из Git. Можно открыть `example.txt` и вручную переключиться на ветку `example-branch`. Перед повторным запуском переместите или удалите `demo/`: существующие файлы PoC не перезаписывает. Путь к папке не зависит от текущей рабочей директории.

Все операции локальные. HTTPS/SSH отключены в Cargo features.
