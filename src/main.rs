// SaveHub запускается без консольного окна, в том числе из ярлыка в dev-сборке.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use savehub::app::app;

// TODO: создает и запускает app
fn main() {
    app::start();
}
