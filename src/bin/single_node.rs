use common::TIMEOUT;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    std_init, Command, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    let dockerfile = "./dockerfiles/single_node.dockerfile";
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let entrypoint = "single_node_entrypoint";

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
            &[("./logs", "/logs")],
            &format!("./target/{container_target}/release/{entrypoint}"),
            &[],
        )],
        false,
        logs_dir,
    );
    cn.run(true).await?;

    let ids = cn.get_ids();
    cn.wait_with_timeout(ids, TIMEOUT).await.unwrap();
    Ok(())
}
