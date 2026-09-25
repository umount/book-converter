//! Exercises the real isolated renderer without ONNX weights, a runtime or API calls.
#![cfg(all(feature = "onnx", feature = "worker"))]
use manga_inference::{
    protocol::{Asset, Operation, Request},
    text::TextRegion,
    worker, Crop,
};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
#[tokio::test]
async fn renders_without_loading_models_and_preserves_source() {
    let root = std::env::temp_dir().join(format!("manga-lettering-test-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let source = root.join("source.png");
    image::RgbImage::from_pixel(400, 300, image::Rgb([230, 235, 240]))
        .save(&source)
        .unwrap();
    let bytes = std::fs::read(&source).unwrap();
    let request = Request {
        version: 1,
        runtime: root.join("absent-runtime"),
        model: root.join("absent-model"),
        input: Asset {
            path: source.clone(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        },
        output: root.join("rendered.png"),
        operation: Operation::Lettering {
            regions: vec![TextRegion {
                id: "dialogue".into(),
                bounds: Crop {
                    x: 40,
                    y: 50,
                    width: 260,
                    height: 140,
                },
                text: "Привет, мир!\nПродолжаем перевод.".into(),
            }],
        },
    };
    let response = worker::execute(
        Path::new(env!("CARGO_BIN_EXE_manga-inference")),
        &request,
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    assert_eq!((response.width, response.height), (400, 300));
    assert_eq!(response.layouts.len(), 1);
    assert_eq!(response.layouts[0].id, "dialogue");
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    let original = image::open(&source).unwrap().to_rgb8();
    let output = image::open(&request.output).unwrap().to_rgb8();
    assert_ne!(original, output);
    for (x, y, pixel) in output.enumerate_pixels() {
        if !(40..300).contains(&x) || !(50..190).contains(&y) {
            assert_eq!(pixel, original.get_pixel(x, y));
        }
    }
}
