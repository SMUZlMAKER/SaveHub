//! Урезает ludusavi-manifest до `data/saves.json`: оставляет только то, что нужно,
//! чтобы по папке игры найти её сохранения на Windows.
//!
//! ```text
//! curl -L -o manifest.yaml https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/refs/heads/data/data/manifest.yaml
//! cargo run --release --example trim -- manifest.yaml data/saves.json
//! ```

#[derive(serde::Serialize)]
struct Entry {
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    steam: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dirs: Vec<String>,
    saves: Vec<String>,
}
use serde::Deserialize;
use serde::de::IgnoredAny;
use std::collections::{BTreeMap, HashMap};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Game {
    files: BTreeMap<String, FileRule>,
    install_dir: BTreeMap<String, IgnoredAny>,
    steam: Steam,
    id: Ids,
    alias: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct FileRule {
    tags: Vec<String>,
    when: Vec<When>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct When {
    os: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Steam {
    id: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Ids {
    steam_extra: Vec<u32>,
}

/// Плейсхолдеры, которые умеет подставлять `save_suggest`. Правила с другими
/// (`<root>` — папка лаунчера, `<game>`, `<storeGameId>`, `xdg*`) отбрасываем.
const SUPPORTED: &[&str] = &[
    "base",
    "home",
    "winAppData",
    "winLocalAppData",
    "winLocalAppDataLow",
    "winDocuments",
    "winPublic",
    "winProgramData",
    "winDir",
    "osUserName",
    "storeUserId",
];

/// Папки, которые нельзя предлагать как «папку с сохранениями» целиком:
/// правило вида `<winDocuments>/*.sav` превратилось бы в весь профиль.
const BROAD: &[&str] = &[
    "<base>",
    "<home>",
    "<home>/appdata",
    "<home>/appdata/local",
    "<home>/appdata/locallow",
    "<home>/appdata/roaming",
    "<home>/documents",
    "<home>/saved games",
    "<winappdata>",
    "<winlocalappdata>",
    "<winlocalappdatalow>",
    "<windocuments>",
    "<winpublic>",
    "<winprogramdata>",
    "<windir>",
];

fn placeholders(path: &str) -> impl Iterator<Item = &str> {
    path.split('<')
        .skip(1)
        .filter_map(|s| s.split_once('>').map(|(name, _)| name))
}

fn is_pattern(part: &str) -> bool {
    part.contains(['*', '?', '['])
}

/// Правило годится: сохранения, Windows, известные плейсхолдеры, не слишком широкое.
fn keep(path: &str, rule: &FileRule) -> bool {
    let is_save = rule.tags.is_empty() || rule.tags.iter().any(|t| t == "save");
    let windows = rule.when.is_empty()
        || rule
            .when
            .iter()
            .any(|w| w.os.as_deref().is_none_or(|os| os == "windows"));
    let supported = placeholders(path).all(|p| SUPPORTED.contains(&p));
    if !(is_save && windows && supported && path.starts_with('<')) {
        return false;
    }

    // Папка, которую в итоге предложим: если последний компонент похож на файл
    // или шаблон, это его родитель. Её литеральная часть не должна быть слишком общей.
    let parts: Vec<&str> = path.split('/').collect();
    let last = parts.last().unwrap();
    let dir = if parts.len() > 1 && (is_pattern(last) || last.contains('.')) {
        &parts[..parts.len() - 1]
    } else {
        &parts[..]
    };
    let literal: Vec<&str> = dir.iter().take_while(|p| !is_pattern(p)).copied().collect();
    !BROAD.contains(&literal.join("/").to_lowercase().as_str())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: trim <manifest.yaml> <saves.json>");
        std::process::exit(2);
    };

    let text = std::fs::read_to_string(input).expect("чтение манифеста");
    let manifest: HashMap<String, Game> = serde_yaml::from_str(&text).expect("разбор манифеста");

    let mut entries: Vec<Entry> = manifest
        .into_iter()
        .filter(|(_, game)| game.alias.is_none())
        .filter_map(|(name, game)| {
            let saves: Vec<String> = game
                .files
                .iter()
                .filter(|(path, rule)| keep(path, rule))
                .map(|(path, _)| path.clone())
                .collect();
            (!saves.is_empty()).then(|| Entry {
                name,
                steam: game
                    .steam
                    .id
                    .into_iter()
                    .chain(game.id.steam_extra)
                    .collect(),
                dirs: game.install_dir.into_keys().collect(),
                saves,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let json = serde_json::to_string(&entries).unwrap();
    std::fs::write(output, &json).expect("запись результата");
    let rules: usize = entries.iter().map(|e| e.saves.len()).sum();
    println!(
        "{} игр, {} правил, {:.1} МБ -> {output}",
        entries.len(),
        rules,
        json.len() as f64 / 1e6
    );
}
