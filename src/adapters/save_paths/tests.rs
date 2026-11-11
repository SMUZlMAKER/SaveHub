//! Тесты подсказки папки с сохранениями на маленькой базе и временных папках.

use super::{SaveDb, key};
use std::fs;
use std::path::{Path, PathBuf};

const DB: &str = r#"[
    {"name": "S.T.A.L.K.E.R.: Call of Pripyat", "steam": [41700],
     "dirs": ["Stalker Call of Pripyat"], "saves": ["<base>/_appdata_/savedgames"]},
    {"name": "Some Game", "dirs": ["SomeGame"], "saves": ["<base>/Saves/*.sav"]}
]"#;

/// Временная папка с файлами: `files` — пути относительно неё.
fn sandbox(test: &str, files: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("save_suggest_{test}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for file in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "41700").unwrap();
    }
    root
}

#[test]
fn key_ignores_punctuation_and_case() {
    assert_eq!(
        key("S.T.A.L.K.E.R. - Call of Pripyat"),
        key("Stalker Call of Pripyat")
    );
}

#[test]
fn finds_saves_by_folder_name_when_exe_is_in_bin() {
    let root = sandbox(
        "folder",
        &[
            "S.T.A.L.K.E.R. - Call of Pripyat/bin/xrEngine.exe",
            "S.T.A.L.K.E.R. - Call of Pripyat/_appdata_/savedgames/a.sav",
        ],
    );
    let db = SaveDb::from_json(DB).unwrap();
    let s = db
        .suggest(&root.join("S.T.A.L.K.E.R. - Call of Pripyat/bin/xrEngine.exe"))
        .unwrap();
    assert_eq!(s.game, "S.T.A.L.K.E.R.: Call of Pripyat");
    assert_eq!(
        s.save_dirs,
        [root.join("S.T.A.L.K.E.R. - Call of Pripyat/_appdata_/savedgames")]
    );
}

#[test]
fn finds_game_by_steam_app_id_in_renamed_folder() {
    let root = sandbox(
        "appid",
        &[
            "my stalker/bin/steam_appid.txt",
            "my stalker/bin/xrEngine.exe",
        ],
    );
    let db = SaveDb::from_json(DB).unwrap();
    let s = db
        .suggest(&root.join("my stalker/bin/xrEngine.exe"))
        .unwrap();
    assert_eq!(s.game, "S.T.A.L.K.E.R.: Call of Pripyat");
    assert!(
        s.save_dirs.is_empty(),
        "игру ещё не запускали — сохранений нет"
    );
}

#[test]
fn file_rule_suggests_its_folder() {
    let root = sandbox(
        "file",
        &[
            "SomeGame/game.exe",
            "SomeGame/Saves/slot1.sav",
            "SomeGame/Saves/slot2.sav",
        ],
    );
    let db = SaveDb::from_json(DB).unwrap();
    let s = db.suggest(&root.join("SomeGame/game.exe")).unwrap();
    assert_eq!(s.save_dirs, [root.join("SomeGame/Saves")]);
}

#[test]
fn steam_library_game_by_appmanifest() {
    let root = sandbox(
        "library",
        &[
            "steamapps/common/Pripyat/bin/xrEngine.exe",
            "steamapps/common/Pripyat/_appdata_/savedgames/a.sav",
        ],
    );
    fs::write(
        root.join("steamapps/appmanifest_41700.acf"),
        "\"AppState\"\n{\n\t\"appid\"\t\t\"41700\"\n\t\"installdir\"\t\t\"Pripyat\"\n}\n",
    )
    .unwrap();
    let db = SaveDb::from_json(DB).unwrap();
    let s = db
        .suggest(&root.join("steamapps/common/Pripyat/bin/xrEngine.exe"))
        .unwrap();
    assert_eq!(s.game, "S.T.A.L.K.E.R.: Call of Pripyat");
    assert_eq!(
        s.save_dirs,
        [root.join("steamapps/common/Pripyat/_appdata_/savedgames")]
    );
}

#[test]
fn unknown_game() {
    let db = SaveDb::from_json(DB).unwrap();
    assert!(db.suggest(Path::new("/tmp/Unknown/game.exe")).is_none());
}
