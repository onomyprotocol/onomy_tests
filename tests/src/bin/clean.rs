//! cleans out all log files and resources that aren't permanent

use onomy_test_lib::super_orchestrator::{remove_files_in_dir, stacked_errors::Result, std_init};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    remove_files_in_dir("./tests/dockerfiles", &["__tmp.dockerfile"]).await?;
    remove_files_in_dir("./tests/dockerfiles/dockerfile_resources", &[
        "__tmp_hermes_config.toml",
        "onomyd",
        "marketd",
        "onexd",
        "gravity",
        "arc_ethd",
        "interchain-security-cd",
    ])
    .await?;
    remove_files_in_dir("./tests/logs", &[".log", ".json", ".toml"]).await?;
    remove_files_in_dir("./tests/resources/keyring-test/", &[".address", ".info"]).await?;
    remove_files_in_dir("./tests/resources/tmp", &[".log", ".json", ".toml"]).await?;

    Ok(())
}
