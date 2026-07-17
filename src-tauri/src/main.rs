//! Бинарная точка входа Tauri-приложения.
//! Вся логика — в библиотеке `book_converter_lib` (см. `lib.rs`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    book_converter_lib::run();
}
