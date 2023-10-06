use anyhow::bail;
use bollard::exec;
use bollard::Docker;
use futures_util::TryStreamExt;

pub(crate) async fn exec(
    docker: &Docker,
    container_name: &str,
    cmd: &str,
    working_dir: Option<&str>,
) -> anyhow::Result<String> {
    let error_code = uuid::Uuid::new_v4();
    let command = format!("{} || echo {}", cmd, error_code);

    let exec_config = exec::CreateExecOptions {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        cmd: Some(vec!["/bin/sh", "-c", command.as_str()]),
        working_dir,
        ..Default::default()
    };

    let exec = docker.create_exec(container_name, exec_config).await?;
    let output = if let exec::StartExecResults::Attached { output, .. } =
        docker.start_exec(&exec.id, None).await?
    {
        output
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .map(|msg| msg.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        unreachable!();
    };

    if output.contains(&error_code.to_string()) {
        bail!(
            "Failed to execute command: {}\n{}",
            cmd,
            output.replace(&error_code.to_string(), "")
        );
    }

    Ok(output)
}

/// export a directory from a container in tar format
pub(crate) async fn export(
    docker: &Docker,
    container_name: &str,
    dir: &str,
) -> anyhow::Result<Vec<u8>> {
    let command = format!("tar -C {} -cf - .", dir);

    let exec_config = exec::CreateExecOptions {
        attach_stdout: Some(true),
        cmd: Some(vec!["/bin/sh", "-c", command.as_str()]),
        ..Default::default()
    };

    let exec = docker.create_exec(container_name, exec_config).await?;
    let output = if let exec::StartExecResults::Attached { output, .. } =
        docker.start_exec(&exec.id, None).await?
    {
        output
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flat_map(|msg| msg.into_bytes())
            .collect::<Vec<_>>()
    } else {
        unreachable!();
    };

    Ok(output)
}
