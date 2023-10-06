mod exec;

use anyhow::Result;
pub use bollard;
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::TryStreamExt;
use std::collections::HashMap;
use std::io::Write;
use std::{fs, path};

#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub image: String,
    pub hostname: String,
    pub working_dir: String,
    pub zip_name: String,
    pub timeout: i64,
    pub memory_limit: i64,
    pub cpu_limit: i64,
    pub disk_limit: String,
    pub extract: Option<String>,
    pub debug: bool,
}

impl Default for CheckConfig {
    fn default() -> Self {
        Self {
            image: "buildpack-deps:stable".to_string(),
            hostname: "canary".to_string(),
            working_dir: "/homework".to_string(),
            zip_name: "homework.zip".to_string(),
            timeout: 90,
            memory_limit: 1024 * 1024 * 1024,
            cpu_limit: 1,
            disk_limit: "1G".to_string(),
            extract: None,
            debug: false,
        }
    }
}

#[derive(Debug)]
pub enum CheckError {
    ImagePullError,
    ContainerCreateError { output: String },
    ContainerStartError { output: String },
    UnzipError { output: String },
    MakeError { output: String },
    FindError { output: String },
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CheckError::ImagePullError => write!(f, "Failed to pull image"),
            CheckError::ContainerCreateError { output } => {
                write!(f, "Failed to create container: {}", output)
            }
            CheckError::ContainerStartError { output } => {
                write!(f, "Failed to start container: {}", output)
            }
            CheckError::UnzipError { output } => write!(f, "Failed to unzip: {}", output),
            CheckError::MakeError { output } => write!(f, "Failed to make: {}", output),
            CheckError::FindError { output } => write!(f, "Failed to find executables: {}", output),
        }
    }
}

impl std::error::Error for CheckError {}

pub async fn run_checks(
    docker: &Docker,
    zip_path: &str,
    config: CheckConfig,
) -> Result<Vec<String>> {
    let zip_path = std::fs::canonicalize(zip_path)?;
    let zip_path = zip_path.to_str().unwrap();

    let create_image_options = CreateImageOptions {
        from_image: config.image.clone(),
        ..Default::default()
    };
    docker
        .create_image(Some(create_image_options), None, None)
        .try_collect::<Vec<_>>()
        .await
        .map_err(|_| CheckError::ImagePullError)?;

    let mut tmpfs: HashMap<String, String> = HashMap::new();
    tmpfs.insert(
        "/homework".to_string(),
        format!("rw,noexec,nosuid,size={}", config.disk_limit),
    );

    // Define container configuration
    let container_config = Config {
        image: Some(config.image.clone()),
        cmd: Some(vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "while true; do sleep 10; done".to_string(),
        ]),
        hostname: Some(config.hostname.clone()),
        network_disabled: Some(true),
        working_dir: Some(config.working_dir.clone()),
        stop_timeout: Some(config.timeout),
        host_config: Some(HostConfig {
            cpu_period: Some(100000),
            cpu_quota: Some(config.cpu_limit * 100000),
            memory: Some(config.memory_limit),
            memory_swap: Some(config.memory_limit),
            binds: Some(vec![format!(
                "{}:{}",
                zip_path,
                format!("{}/{}", &config.working_dir, &config.zip_name)
            )]),
            readonly_rootfs: Some(true),
            tmpfs: Some(tmpfs),
            ..Default::default()
        }),
        tty: Some(true),
        ..Default::default()
    };

    // Create a new container
    let container_name = format!("canary-checker-{}", uuid::Uuid::new_v4());
    docker
        .create_container(
            Some(CreateContainerOptions {
                name: container_name.clone(),
                platform: None,
            }),
            container_config,
        )
        .await
        .map_err(|err| CheckError::ContainerCreateError {
            output: err.to_string(),
        })?;

    // Start the container
    docker
        .start_container::<&str>(&container_name, None)
        .await
        .map_err(|err| CheckError::ContainerStartError {
            output: err.to_string(),
        })?;

    let result = run_checks_inner(docker, &container_name, &config).await;

    let _ = docker
        .remove_container(
            &container_name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;

    result
}

async fn run_checks_inner(
    docker: &Docker,
    container_name: &str,
    config: &CheckConfig,
) -> Result<Vec<String>> {
    let working_dir = config.working_dir.clone();

    if config.debug {
        let output =
            crate::exec::exec(docker, container_name, "ls -la", Some(working_dir.as_str())).await?;
        println!("contents:\n{}", output);
    }

    crate::exec::exec(
        docker,
        container_name,
        format!("timeout 30 unzip -j {}", &config.zip_name).as_str(),
        Some(working_dir.as_str()),
    )
    .await
    .map_err(|err| CheckError::UnzipError {
        output: err.to_string(),
    })?;
    if config.debug {
        let output =
            crate::exec::exec(docker, container_name, "ls -la", Some(working_dir.as_str())).await?;
        println!("after unzip:\n{}", output);
    }

    crate::exec::exec(
        docker,
        container_name,
        "timeout 30 make",
        Some(working_dir.as_str()),
    )
    .await
    .map_err(|err| CheckError::MakeError {
        output: err.to_string(),
    })?;
    if config.debug {
        let output =
            crate::exec::exec(docker, container_name, "ls -la", Some(working_dir.as_str())).await?;
        println!("after make:\n{}", output);
    }

    let output = crate::exec::exec(
        docker,
        container_name,
        "timeout 10 find . -type f -perm /111",
        Some(working_dir.as_str()),
    )
    .await
    .map_err(|err| CheckError::FindError {
        output: err.to_string(),
    })?;

    let mut executables: Vec<String> = Vec::new();
    for line in output.lines() {
        let line = if let Some(line) = line.strip_prefix("./") {
            line
        } else {
            line
        };

        if line.starts_with('_') {
            continue;
        }

        executables.push(line.to_string());
    }

    executables.sort_unstable();

    if let Some(dest) = &config.extract {
        let parent = path::Path::new(dest).parent().unwrap();
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }

        let tar = crate::exec::export(docker, container_name, &config.working_dir).await?;
        let file = fs::File::create(dest)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(&tar)?;
    }

    Ok(executables)
}
