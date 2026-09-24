//! Developer probe and isolated worker entry point. No downloads or API calls.
use manga_inference::{onnx, validate_dimensions, Crop};
use std::{io::Write, path::Path};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 && args.len() != 6 {
        return Err(
            "usage: manga-inference mask|probabilities|clean RUNTIME MODEL INPUT OUTPUT [MASK]"
                .into(),
        );
    }
    let started = std::time::Instant::now();
    onnx::initialize(Path::new(&args[1]))?;
    let reader = image::ImageReader::open(&args[3])?.with_guessed_format()?;
    let dimensions = reader.into_dimensions()?;
    validate_dimensions(dimensions.0, dimensions.1)?;
    if dimensions.0 > 2048 || dimensions.1 > 2048 {
        return Err("probe input exceeds crop budget".into());
    }
    let original = image::open(&args[3])?.to_rgb8();
    let loaded;
    match args[0].as_str() {
        "mask" | "probabilities" => {
            let mut model = onnx::TextMask::load(Path::new(&args[2]))?;
            loaded = started.elapsed();
            if args[0] == "mask" {
                model
                    .segment(
                        &original,
                        Crop {
                            x: 0,
                            y: 0,
                            width: original.width(),
                            height: original.height(),
                        },
                    )?
                    .save(&args[4])?;
            } else {
                let values = model.probabilities(&original)?;
                let mut file = std::fs::File::create(&args[4])?;
                for value in values {
                    file.write_all(&value.to_le_bytes())?;
                }
            }
        }
        "clean" => {
            let mask_path = args.get(5).ok_or("mask file required")?;
            let mut reader = image::ImageReader::open(mask_path)?.with_guessed_format()?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(2048);
            limits.max_image_height = Some(2048);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            let mask = reader.decode()?.to_luma8();
            let mut model = onnx::Lama::load(Path::new(&args[2]))?;
            loaded = started.elapsed();
            model.clean(&original, &mask)?.save(&args[4])?;
        }
        _ => return Err("unknown operation".into()),
    }
    println!(
        "{}",
        serde_json::json!({"loadMillis":loaded.as_millis(),"inferenceMillis":started.elapsed().as_millis()-loaded.as_millis()})
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
