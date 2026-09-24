//! Explicit maintenance tool; never called on application startup.
//! Usage: reset_legacy <app-data-directory> [--apply --app-stopped]
//! The caller must close the application and verify no writers remain before applying.
use book_converter_lib::project::lifecycle::ProjectManager;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = args
        .first()
        .filter(|v| !v.starts_with('-'))
        .ok_or_else(|| anyhow::anyhow!("Expected an explicit app-data directory"))?;
    let manager = ProjectManager::new(std::path::PathBuf::from(root));
    let report = if args.iter().any(|v| v == "--apply") {
        anyhow::ensure!(
            args.iter().any(|v| v == "--app-stopped"),
            "Stop the application and pass --app-stopped"
        );
        manager
            .execute_reset(|_| Ok(()))
            .map_err(|e| anyhow::anyhow!("Reset failed: {e:?}"))?
    } else {
        manager
            .reset_candidates()
            .map_err(|e| anyhow::anyhow!("Inspection failed: {e:?}"))?
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
