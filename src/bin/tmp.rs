// for temporary tests

use super_orchestrator::{remove_files_in_dir, std_init, Result};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    remove_files_in_dir("./logs", &["log"]).await?;

    Ok(())
}
