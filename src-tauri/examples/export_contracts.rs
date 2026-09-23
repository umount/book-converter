//! Generate or check canonical frontend wire types without launching Tauri.
fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/shared/contracts/generated.ts");
    let expected = book_converter_lib::app::contracts::typescript();
    if std::env::args().any(|a| a == "--check") {
        let actual = std::fs::read_to_string(&path).unwrap_or_default();
        if actual != expected {
            eprintln!("Contract drift: run npm run contracts:generate");
            std::process::exit(1);
        }
    } else {
        std::fs::write(path, expected).expect("write generated contracts");
    }
}
