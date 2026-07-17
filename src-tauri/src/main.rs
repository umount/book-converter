//! Binary entry point of the Tauri application.
//! All logic lives in the `book_converter_lib` library (see `lib.rs`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    book_converter_lib::run();
}
