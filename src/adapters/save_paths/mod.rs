//! Подсказка папки с сохранениями по exe игры.
//!
//! Пользователь выбирает exe игры, мы находим игру в базе и предлагаем папки
//! с её сохранениями, которые реально есть на диске.
//!
//! База — урезанный ludusavi-manifest (`data/saves.json`, собирается `tools/save-manifest/trim.rs`):
//! для каждой игры только Windows-пути к сохранениям, имена папок установки и Steam app id.

use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// Запись базы.
#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steam: Vec<u32>,
    /// имена папок установки
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirs: Vec<String>,
    /// пути к сохранениям с плейсхолдерами: `<base>/saves`, `<winAppData>/Game`
    pub saves: Vec<String>,
}

#[derive(Debug)]
pub struct Suggestion {
    pub game: String,
    /// существующие папки с сохранениями (может быть пусто, если игру ещё не запускали)
    pub save_dirs: Vec<PathBuf>,
}

pub struct SaveDb {
    games: Vec<Entry>,
    by_steam_id: HashMap<u32, usize>,
    by_name: HashMap<String, usize>,
}

/// Короткие ключи (`bin`, `x64`) дают ложные совпадения.
const MIN_KEY: usize = 5;
/// Сколько папок над exe проверять: exe бывает в `bin/` или `Binaries/Win64/`.
const SEARCH_UP: usize = 4;

/// Ключ для сравнения названий: только буквы и цифры в нижнем регистре.
/// `S.T.A.L.K.E.R. - Call of Pripyat` и `Stalker Call of Pripyat` дают один ключ.
pub fn key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl SaveDb {
    /// База, вшитая в бинарник.
    pub fn bundled() -> Self {
        Self::from_json(include_str!("saves.json")).expect("data/saves.json повреждён")
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let games: Vec<Entry> = serde_json::from_str(json)?;
        let mut by_steam_id = HashMap::new();
        let mut by_name = HashMap::new();
        let mut index = |name: &str, i: usize| {
            let key = key(name);
            if key.chars().count() >= MIN_KEY {
                by_name.entry(key).or_insert(i);
            }
        };
        // сначала названия, потом папки установки: точное название важнее
        for (i, game) in games.iter().enumerate() {
            index(&game.name, i);
            for id in &game.steam {
                by_steam_id.entry(*id).or_insert(i);
            }
        }
        for (i, game) in games.iter().enumerate() {
            game.dirs.iter().for_each(|dir| index(dir, i));
        }
        Ok(SaveDb {
            games,
            by_steam_id,
            by_name,
        })
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// Находит игру по пути к её exe и папки с сохранениями.
    /// `None`, если игры нет в базе.
    pub fn suggest(&self, exe: &Path) -> Option<Suggestion> {
        // папки над exe, без корня диска
        let folders: Vec<&Path> = exe
            .ancestors()
            .skip(1)
            .take(SEARCH_UP)
            .filter(|p| p.parent().is_some())
            .collect();

        // 1. Steam app id в файлах игры — самый точный признак
        // 2. имя папки (ближайшая к exe важнее)
        let found = folders
            .iter()
            .find_map(|dir| self.by_steam_id.get(&steam_app_id(dir)?))
            .or_else(|| {
                folders.iter().find_map(|dir| {
                    let name = dir.file_name()?.to_string_lossy();
                    self.by_name.get(&key(&name))
                })
            })?;
        let game = &self.games[*found];

        let mut save_dirs = BTreeSet::new();
        for rule in &game.saves {
            if rule.starts_with("<base>") {
                // где именно корень игры, точно не знаем — пробуем папки от exe вверх
                let hit = folders
                    .iter()
                    .map(|base| existing(rule, Some(base)))
                    .find(|d| !d.is_empty());
                save_dirs.extend(hit.unwrap_or_default());
            } else {
                save_dirs.extend(existing(rule, None));
            }
        }
        Some(Suggestion {
            game: game.name.clone(),
            save_dirs: collapse(save_dirs),
        })
    }
}

/// Steam app id для папки игры:
/// - `<id>_install.vdf` (из дистрибутива Steam) или `steam_appid.txt`
///   (рядом с exe или в `steam_settings/`) — остаются и у сборок вне Steam;
/// - для игр из Steam: `steamapps/appmanifest_<id>.acf`, где `installdir` — эта папка.
fn steam_app_id(dir: &Path) -> Option<u32> {
    let from_file = |path: PathBuf| fs::read_to_string(path).ok()?.trim().parse().ok();
    from_file(dir.join("steam_appid.txt"))
        .or_else(|| from_file(dir.join("steam_settings/steam_appid.txt")))
        .or_else(|| {
            fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                name.strip_suffix("_install.vdf")?.parse().ok()
            })
        })
        .or_else(|| steam_library_app_id(dir))
}

/// `<library>/steamapps/common/<dir>` -> id из `<library>/steamapps/appmanifest_<id>.acf`.
fn steam_library_app_id(dir: &Path) -> Option<u32> {
    let common = dir.parent()?;
    let steamapps = common.parent()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name));
    if !is(common, "common") || !is(steamapps, "steamapps") {
        return None;
    }
    let folder = dir.file_name()?.to_string_lossy().to_lowercase();
    fs::read_dir(steamapps).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let id = name
            .strip_prefix("appmanifest_")?
            .strip_suffix(".acf")?
            .parse()
            .ok()?;
        // строка вида:  "installdir"		"DOOM"
        let acf = fs::read_to_string(entry.path()).ok()?;
        let install_dir = acf.lines().find_map(|line| {
            let mut parts = line.split('"').filter(|s| !s.trim().is_empty());
            parts
                .next()?
                .eq_ignore_ascii_case("installdir")
                .then(|| parts.next())?
        })?;
        (install_dir.to_lowercase() == folder).then_some(id)
    })
}

/// Значение плейсхолдера, уже экранированное для glob.
fn placeholder(name: &str, base: Option<&Path>) -> Option<String> {
    let path = |p: Option<PathBuf>| p.map(|p| Pattern::escape(&p.to_string_lossy()));
    let env = |var: &str| std::env::var_os(var).map(PathBuf::from);
    let home = dirs::home_dir();
    match name {
        "base" => path(base.map(Path::to_path_buf)),
        "home" => path(home),
        // config_dir на Windows = Roaming AppData
        "winAppData" => path(dirs::config_dir()),
        "winLocalAppData" => path(dirs::data_local_dir()),
        "winLocalAppDataLow" => path(home.map(|h| h.join("AppData").join("LocalLow"))),
        // учитывает перенос «Документов» в OneDrive
        "winDocuments" => path(dirs::document_dir()),
        "winPublic" => path(dirs::public_dir().or_else(|| env("PUBLIC"))),
        "winProgramData" => path(env("PROGRAMDATA")),
        "winDir" => path(env("WINDIR")),
        "osUserName" => std::env::var("USERNAME").ok().map(|u| Pattern::escape(&u)),
        // ID аккаунта в магазине — подходит любой
        "storeUserId" => Some("*".into()),
        _ => None,
    }
}

/// Подставляет плейсхолдеры и возвращает существующие папки с сохранениями.
fn existing(rule: &str, base: Option<&Path>) -> Vec<PathBuf> {
    let mut pattern = String::new();
    let mut rest = rule;
    while let Some((before, after)) = rest.split_once('<') {
        let Some((name, after)) = after.split_once('>') else {
            return Vec::new();
        };
        let Some(value) = placeholder(name, base) else {
            return Vec::new();
        };
        pattern.push_str(before);
        pattern.push_str(&value);
        rest = after;
    }
    pattern.push_str(rest);

    let options = MatchOptions {
        case_sensitive: false,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let Ok(paths) = glob::glob_with(&pattern.replace('\\', "/"), options) else {
        return Vec::new();
    };
    paths
        .flatten()
        // правило может указывать на файл — тогда берём его папку
        .filter_map(|p| {
            if p.is_dir() {
                Some(p)
            } else {
                p.parent().map(Path::to_path_buf)
            }
        })
        .map(|p| {
            PathBuf::from(
                p.to_string_lossy()
                    .replace('/', std::path::MAIN_SEPARATOR_STR),
            )
        })
        .collect()
}

/// Убирает папки, вложенные в другие папки из набора.
fn collapse(dirs: BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !result.iter().any(|parent| dir.starts_with(parent)) {
            result.push(dir);
        }
    }
    result
}

#[cfg(test)]
mod tests;
