use onomy_test_lib::{
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        sh,
        stacked_errors::Result,
    },
    Args, TIMEOUT,
};

/// Useful for running simple container networks that have a standard format and
/// don't need extra build or volume arguments.
pub async fn container_runner(
    args: &Args,
    dockerfiles_and_entry_names: &[(&str, &str)],
) -> Result<()> {
    let bin_entrypoint = &args.bin_name;
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./tests/logs";

    // build internal runner
    sh("cargo build --release --bin", &[
        bin_entrypoint,
        "--target",
        container_target,
    ])
    .await?;

    let mut cn = ContainerNetwork::new(
        "test",
        dockerfiles_and_entry_names
            .iter()
            .map(|(dockerfile, entry_name)| {
                Container::new(
                    entry_name,
                    Dockerfile::Path(format!("./tests/dockerfiles/{dockerfile}.dockerfile")),
                    Some(&format!(
                        "./target/{container_target}/release/{bin_entrypoint}"
                    )),
                    &["--entry-name", entry_name],
                )
            })
            .collect(),
        Some("./dockerfiles"),
        true,
        logs_dir,
    )?
    .add_common_volumes(&[(logs_dir, "/logs")]);
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await.unwrap();
    Ok(())
}
