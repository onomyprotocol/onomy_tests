use common::TIMEOUT;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    sh, std_init, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    let dockerfile = "./dockerfiles/provider.dockerfile";
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let entrypoint = "ics_basic_entrypoint";

    // build internal runner
    sh("cargo build --release --bin", &[
        entrypoint,
        "--target",
        container_target,
    ])
    .await?;

    // build binary
    //sh("make --directory ./../onomy_workspace0/onomy/ build", &[]).await?;
    // copy (docker cannot use files from outside cwd)
    sh(
        "cp ./../onomy_workspace0/onomy/onomyd ./dockerfiles/dockerfile_resources/onomyd",
        &[],
    )
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
    cn.wait_with_timeout(ids, TIMEOUT).await.unwrap();
    Ok(())
}
