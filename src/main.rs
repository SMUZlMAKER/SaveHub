// В релизной сборке под Windows не открываем консольное окно рядом с GUI
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use savehub::app::app;

// TODO: создает и запускает app
fn main() {
    app::start();
}
