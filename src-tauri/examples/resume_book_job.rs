//! Resume one saved book job through the same pipeline as the desktop app.
use book_converter_lib::{app::contracts::ProjectId, application::runtime, jobs::durable, project::lifecycle::ProjectManager};
use std::sync::{Arc, atomic::AtomicBool};
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "Usage: resume_book_job APP_DATA PROJECT_ID JOB_ID");
    let manager = ProjectManager::new(args[1].clone().into());
    let project: ProjectId = serde_json::from_value(serde_json::json!(args[2])).expect("project ID");
    let result = async {
        let pipeline = runtime::resume_provider(&manager, &project, &args[3])?;
        durable::execute(&manager, &project, &args[3], &pipeline, Arc::new(AtomicBool::new(false)), |_| {}).await
    }.await;
    match result {
        Ok(()) => println!("Job completed"),
        Err(error) => { eprintln!("{}", serde_json::to_string(&error).unwrap()); std::process::exit(1); }
    }
}
