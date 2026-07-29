use std::{io::Write as _, path::PathBuf, process::ExitCode};

use qingyu_kernel::api::{check_openapi_artifact, export_openapi_to_string};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), &'static str> {
    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("openapi")
        .join("kernel-v1.json");
    match std::env::args().nth(1).as_deref() {
        Some("--check") => {
            check_openapi_artifact(&artifact).map_err(|_| "OpenAPI artifact drift detected.")
        }
        Some("--write") => {
            let output = export_openapi_to_string().map_err(|_| "OpenAPI export failed.")?;
            std::fs::create_dir_all(
                artifact
                    .parent()
                    .expect("the artifact has an OpenAPI directory"),
            )
            .map_err(|_| "OpenAPI directory creation failed.")?;
            std::fs::write(artifact, output).map_err(|_| "OpenAPI artifact write failed.")
        }
        None => {
            let output = export_openapi_to_string().map_err(|_| "OpenAPI export failed.")?;
            std::io::stdout()
                .write_all(output.as_bytes())
                .map_err(|_| "OpenAPI stdout write failed.")
        }
        Some(_) => Err("Usage: export_openapi [--check|--write]"),
    }
}
