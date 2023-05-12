use std::time::Duration;

use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    Command, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let dockerfile = "./dockerfiles/chain_upgrade_test.dockerfile";
    //let dockerfile = "./dockerfiles/onomy_base.dockerfile";
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let entrypoint = "chain_upgrade_test_entrypoint";

    // build internal runner
    Command::new("cargo", &[
        "build",
        "--release",
        "--bin",
        entrypoint,
        "--target",
        container_target,
    ])
    .run_to_completion()
    .await?
    .assert_success()?;

    let mut cn = ContainerNetwork::new(
        "test",
        vec![Container::new(
            "main",
            Some(dockerfile),
            "main_build",
            &[],
            &format!("./target/{container_target}/release/{entrypoint}"),
            &[],
        )],
        false,
        logs_dir,
    );
    cn.run(true).await?;

    let ids = cn.get_ids();
    cn.wait_with_timeout(ids, Duration::from_secs(100))
        .await
        .unwrap();
    Ok(())
}
