use canary_probe::{bollard::Docker, run_checks, CheckConfig, CheckError};

#[tokio::main]
async fn main() {
    let docker = Docker::connect_with_local_defaults();
    if docker.is_err() {
        println!("Docker is unavailable");
        return;
    }
    let docker = docker.unwrap();

    let file = std::env::args().find(|arg| arg.ends_with(".zip"));
    if file.is_none() {
        println!("Usage: canary-probe <zip-file>");
        return;
    }
    let file = file.unwrap();

    let mut config: CheckConfig = CheckConfig {
        debug: std::env::args().any(|arg| arg == "--debug"),
        ..CheckConfig::default()
    };
    if let Some(extract) = std::env::args().find(|arg| arg.starts_with("--extract=")) {
        config.extract = Some(extract.split('=').nth(1).unwrap().to_string());
    }

    let result = run_checks(&docker, &file, config).await;
    match result {
        Ok(executables) => {
            println!("executables: {:?}", executables);
        }
        Err(e) => match e.downcast_ref() {
            Some(CheckError::ImagePullError) => {
                println!("Failed to pull image");
            }
            Some(CheckError::ContainerCreateError { output }) => {
                println!("Failed to create container: {}", output);
            }
            Some(CheckError::ContainerStartError { output }) => {
                println!("Failed to start container: {}", output);
            }
            Some(CheckError::UnzipError { output }) => {
                println!("Failed to unzip: {}", output);
            }
            Some(CheckError::MakeError { output }) => {
                println!("Failed to make: {}", output);
            }
            Some(CheckError::FindError { output }) => {
                println!("Failed to find executables: {}", output);
            }
            _ => {
                println!("Unknown error: {:?}", e);
            }
        },
    }
}
