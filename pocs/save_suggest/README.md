# Подсказка папки с сохранениями

PoC для MVP: пользователь, как и раньше, сам выбирает exe игры при добавлении, а мы по этому exe находим игру и подставляем в форму папку с её сохранениями. Искать, где игра хранит сейвы, больше не нужно.

Сам ludusavi не используем: берём только его открытую базу [ludusavi-manifest](https://github.com/mtkennerly/ludusavi-manifest).

```rust
let db = SaveDb::bundled();                 // ~20 мс, база вшита в бинарник
if let Some(s) = db.suggest(&exe_path) {    // < 5 мс
    // s.game — название, s.save_dirs — существующие папки с сохранениями
}
```

Проверка на своих играх:

```bash
cargo run --release --example suggest -- "C:\Games\S.T.A.L.K.E.R. - Call of Pripyat\bin\xrEngine.exe"
```

## Как это работает

1. **База** — `data/saves.json` (2.8 МБ, ~650 КБ в gzip, 19 тыс. игр). Собирается из [ludusavi-manifest](https://github.com/mtkennerly/ludusavi-manifest) примером `trim`. Остаются только Windows-пути к сохранениям с поддерживаемыми плейсхолдерами, имена папок установки и Steam app id. Слишком широкие правила (`<winDocuments>/*.sav` → весь профиль) отбрасываются заранее, поэтому в рантайме проверок нет. Обновить базу:
   ```bash
   curl -L -o manifest.yaml https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/refs/heads/data/data/manifest.yaml
   cargo run --release --example trim -- manifest.yaml data/saves.json
   ```
2. **Поиск игры.** Смотрим до 4 папок над exe (exe бывает в `bin/` или `Binaries/Win64/`):
   - Steam app id: `<id>_install.vdf` или `steam_appid.txt` в папке игры (они остаются и у сборок вне Steam), а для игр из Steam — `steamapps/appmanifest_<id>.acf`;
   - иначе имя папки, сравниваем только буквы и цифры (`S.T.A.L.K.E.R. - Call of Pripyat` = `Stalker Call of Pripyat`).
3. **Сохранения.** Подставляем `<base>` (папку игры), `<winAppData>`, `<winDocuments>` и т.д., раскрываем glob, оставляем существующие папки. Правило на файл даёт его папку, вложенные папки схлопываются.

## Результаты на тестовой машине

| exe | игра | сейвы |
|---|---|---|
| `C:\Games\S.T.A.L.K.E.R. - Call of Pripyat\bin\xrEngine.exe` (вне лаунчеров) | ✅ по `41700_install.vdf` | ✅ `_appdata_\savedgames` |
| `steamapps\common\DOOM\DOOMx64.exe` | ✅ Doom (2016) по appmanifest | ✅ 2 папки в `Saved Games\id Software\DOOM\base` |
| `steamapps\common\Geometry Dash\GeometryDash.exe` | ✅ | ✅ `AppData\Local\GeometryDash` |
| `steamapps\common\Easy Red 2\Easy Red 2.exe` | ✅ | ✅ `AppData\LocalLow\CorvoStudio\Easy Red 2\saves` |
| `C:\Games\Pixel Storm\Enlisted\win64\enlisted.exe` | ❌ в манифесте нет путей к сохранениям | — |

## Почему не полное автоопределение

Был и прототип, который сканирует Steam и папки с играми и находит все игры сразу. Он работает, но для MVP громоздкий: ~870 строк, зависимости serde_yaml, ureq и steamlocate, скачивание 17 МБ YAML и разбор ~0.7 с. Подсказка по exe закрывает главную боль (найти, где лежат сейвы) примерно за 265 строк. Полный поиск можно вернуть позже поверх этой же базы.

## Как встроить в SaveHub

Перенести `src/lib.rs` и `data/saves.json` в адаптер (например, `adapters/save_paths`) и добавить ручку `suggest_save_dirs(exe_path)`. Фронт вызывает её после выбора exe в форме добавления игры. Если нашлась одна папка, подставляем её в поле «папка с сохранениями»; если несколько, даём выбрать. Пользователь в любом случае может поправить путь.

## Ограничения

- Игры, которых нет в манифесте или для которых там нет путей к сохранениям (например, Enlisted), не находятся: пользователь указывает путь сам.
- Не поддерживаются сохранения в реестре и папки, куда сейвы перенаправляют эмуляторы Steam API в неофициальных сборках. Пути последних обычно строятся по Steam app id, поэтому их можно добавить небольшим списком шаблонов поверх базы.
- Лицензия: код ludusavi-manifest под MIT, но данные собраны с PCGamingWiki, у которой, насколько известно, CC BY-NC-SA. Для некоммерческого проекта это подходит; перед коммерческим использованием лицензию нужно перепроверить.
