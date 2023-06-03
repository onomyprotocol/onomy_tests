// for temporary tests

use stacked_errors::Result;
use super_orchestrator::{remove_files_in_dir, std_init};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    remove_files_in_dir("./tests/logs", &["log", "json"]).await?;

    Ok(())
}
