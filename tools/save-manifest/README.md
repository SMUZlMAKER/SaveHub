# База подсказок сохранений

Адаптер `src/adapters/save_paths/` перенесён из PoC PR #10 (`b2e06ea`).
База встроена в приложение; сеть при поиске не нужна. Это подсказки для пользователя,
а не разрешение перемещать найденные каталоги. Игра может использовать несколько
папок; текущая форма поддерживает одну и предупреждает об этом.

Обновление базы из ludusavi-manifest:

```sh
curl -L -o manifest.yaml https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/refs/heads/data/data/manifest.yaml
cargo run --manifest-path tools/save-manifest/Cargo.toml -- manifest.yaml src/adapters/save_paths/saves.json
```

`trim.rs` — отдельный инструмент, YAML-парсер не входит в приложение.
Сгенерированный JSON не редактируется вручную. Фильтрация сохраняет ограничения PoC:
Windows, поддерживаемые плейсхолдеры, исключение слишком широких каталогов.
Источник: https://github.com/mtkennerly/ludusavi-manifest.
Лицензионная оговорка исходного PoC: код манифеста под MIT, данные происходят из
PCGamingWiki; условия использования данных требуют отдельной проверки перед
коммерческим распространением.
