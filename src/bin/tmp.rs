// for temporary tests

use super_orchestrator::{std_init, Result};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;

    Ok(())
}
