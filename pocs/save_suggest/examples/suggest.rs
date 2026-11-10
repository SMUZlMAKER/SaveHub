//! Проверка на живых играх:
//! `cargo run --release --example suggest -- "C:\Games\S.T.A.L.K.E.R. - Call of Pripyat\bin\xrEngine.exe"`

use save_suggest::SaveDb;
use std::path::Path;
use std::time::Instant;

fn main() {
    let started = Instant::now();
    let db = SaveDb::bundled();
    println!(
        "База: {} игр, загружена за {:.1?}\n",
        db.len(),
        started.elapsed()
    );

    for exe in std::env::args().skip(1) {
        let started = Instant::now();
        let suggestion = db.suggest(Path::new(&exe));
        let elapsed = started.elapsed();
        println!("{exe}  ({elapsed:.1?})");
        match suggestion {
            None => {
                println!("   не найдено: игры нет в манифесте или для неё нет путей к сохранениям")
            }
            Some(s) if s.save_dirs.is_empty() => {
                println!("   {}: сохранений на диске пока нет", s.game)
            }
            Some(s) => {
                println!("   {}", s.game);
                s.save_dirs
                    .iter()
                    .for_each(|d| println!("   сейвы: {}", d.display()));
            }
        }
    }
}
