use common::TIMEOUT;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    sh, std_init, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    let dockerfile = "./dockerfiles/chain_upgrade_test.dockerfile";
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let entrypoint = "chain_upgrade_test_entrypoint";

    // build internal runner
    sh("cargo build --release --bin", &[
        entrypoint,
        "--target",
        container_target,
    ])
    .await?;

    let mut cn = ContainerNetwork::new(
        "test",
        vec![Container::new(
            "main",
            Some(dockerfile),
            "main_build",
            &[],
            &[("./logs", "/logs")],
            &format!("./target/{container_target}/release/{entrypoint}"),
            &[],
        )],
        false,
        logs_dir,
    );
    cn.run(true).await?;

    let ids = cn.get_ids();
    cn.wait_with_timeout(ids, true, TIMEOUT).await.unwrap();
    Ok(())
}
