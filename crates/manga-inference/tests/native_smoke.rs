//! Opt-in real CPU test: provide MANGA_RUNTIME, MANGA_MASK_MODEL, MANGA_LAMA_MODEL.
//! It creates only synthetic temporary assets and never downloads weights.
#![cfg(all(feature = "onnx", feature = "worker"))]
use manga_inference::{
    protocol::{Asset, Operation, Request},
    worker, Crop,
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};
fn asset(path: PathBuf) -> Asset {
    Asset {
        sha256: format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
        path,
    }
}

#[tokio::test]
#[ignore = "requires explicitly configured native runtime and pinned model files"]
async fn native_mask_cleanup_preserve_artwork_and_original_file() {
    let root = std::env::temp_dir().join(format!("manga-native-smoke-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let original = include_bytes!("fixtures/synthetic-dialogue.png");
    std::fs::write(root.join("original.png"), original).unwrap();
    let mut request = Request {
        version: 1,
        runtime: std::env::var_os("MANGA_RUNTIME")
            .expect("MANGA_RUNTIME")
            .into(),
        model: std::env::var_os("MANGA_MASK_MODEL")
            .expect("MANGA_MASK_MODEL")
            .into(),
        input: asset(root.join("original.png")),
        output: root.join("mask.png"),
        operation: Operation::Masks {
            regions: vec![Crop {
                x: 0,
                y: 0,
                width: 384,
                height: 384,
            }],
            margin: 2,
        },
    };
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_manga-inference"));
    let mask_stats = worker::execute(&worker_path, &request, Duration::from_secs(90))
        .await
        .unwrap();
    let mask = image::open(&request.output).unwrap().to_luma8();
    assert!(mask.as_raw().contains(&255));
    assert!(mask.as_raw().contains(&0));
    request.operation = Operation::Inpainting {
        mask: asset(request.output.clone()),
    };
    request.output = root.join("clean.png");
    request.model = std::env::var_os("MANGA_LAMA_MODEL")
        .expect("MANGA_LAMA_MODEL")
        .into();
    let clean_stats = worker::execute(&worker_path, &request, Duration::from_secs(90))
        .await
        .unwrap();
    let before = image::load_from_memory(original).unwrap().to_rgb8();
    let after = image::open(&request.output).unwrap().to_rgb8();
    assert_eq!(before.dimensions(), after.dimensions());
    let mut changes = 0;
    for (x, y, pixel) in before.enumerate_pixels() {
        if mask.get_pixel(x, y)[0] == 0 {
            assert_eq!(pixel, after.get_pixel(x, y));
        } else if pixel != after.get_pixel(x, y) {
            changes += 1;
        }
    }
    assert!(changes > 0);
    assert_eq!(std::fs::read(&request.input.path).unwrap(), original);
    // Existing result paths cannot be overwritten on rerun.
    assert!(
        worker::execute(&worker_path, &request, Duration::from_secs(90))
            .await
            .is_err()
    );
    eprintln!("mask: {mask_stats:?}; cleanup: {clean_stats:?}");
}
