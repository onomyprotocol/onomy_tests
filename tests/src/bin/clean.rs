// for temporary tests

use onomy_test_lib::super_orchestrator::{
    acquire_file_path, remove_files_in_dir, stacked_errors::Result, std_init,
};
use tokio::fs::remove_file;

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    remove_file(acquire_file_path("./tests/dockerfiles/__tmp.dockerfile").await?).await?;
    remove_files_in_dir("./tests/dockerfiles/dockerfile_resources", &[
        "onomyd",
        "marketd",
        "market_standaloned",
        "gravity",
        "arc_ethd",
        "interchain-security-cd",
    ])
    .await?;
    remove_files_in_dir("./tests/logs", &["log", "json"]).await?;
    remove_files_in_dir("./tests/resources/keyring-test/", &["address", "info"]).await?;

    Ok(())
}
