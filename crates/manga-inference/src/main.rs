//! Developer probe and isolated worker entry point. No downloads or API calls.
use manga_inference::{onnx, validate_dimensions, Crop};
use std::{
    io::{Read, Write},
    path::Path,
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["worker"] {
        return worker();
    }
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

fn worker() -> Result<(), Box<dyn std::error::Error>> {
    use manga_inference::{
        page,
        protocol::{self, Operation, Request, Response},
    };
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(protocol::MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > protocol::MAX_REQUEST_BYTES {
        return Err("request too large".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    request.validate()?;
    if std::fs::symlink_metadata(&request.output).is_ok() {
        return Err("output already exists".into());
    }
    let started = std::time::Instant::now();
    let original = request.input.read()?.to_rgb8();
    onnx::initialize(&request.runtime)?;
    let loaded;
    let output = match request.operation {
        Operation::Masks { regions, margin } => {
            let mut model = onnx::TextMask::load(&request.model)?;
            loaded = started.elapsed();
            image::DynamicImage::ImageLuma8(page::segment_page(
                &original,
                &regions,
                margin,
                |page, crop| model.segment(page, crop),
            )?)
        }
        Operation::Inpainting { mask } => {
            let mask = mask.read()?;
            // Reject RGB/alpha masks rather than interpreting arbitrary artwork as a mask.
            let mask = mask.as_luma8().ok_or("mask must be grayscale 8-bit")?;
            let mut model = onnx::Lama::load(&request.model)?;
            loaded = started.elapsed();
            image::DynamicImage::ImageRgb8(model.clean(&original, mask)?)
        }
    };
    protocol::save_new(&output, &request.output)?;
    println!(
        "{}",
        serde_json::to_string(&Response {
            version: 1,
            width: output.width(),
            height: output.height(),
            load_millis: loaded.as_millis() as u64,
            inference_millis: (started.elapsed() - loaded).as_millis() as u64,
        })?
    );
    Ok(())
}
