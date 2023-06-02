use clap::Parser;
use onomy_test_lib::{Args, TIMEOUT};
use stacked_errors::{MapAddError, Result};
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    net_message::NetMessenger,
    sh, std_init, STD_DELAY, STD_TRIES,
};

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    let args = Args::parse();

    if let Some(ref s) = args.entrypoint {
        match s.as_str() {
            "tmp0" => tmp0().await,
            "tmp1" => tmp1().await,
            _ => format!("entrypoint \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        container_runner().await
    }
}

async fn container_runner() -> Result<()> {
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let this_bin = "tmp2";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        this_bin,
        "--target",
        container_target,
    ])
    .await?;

    let entrypoint = &format!("./target/{container_target}/release/{this_bin}");
    let volumes = &[("./logs", "/logs"), ("./resources", "/resources")];
    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "tmp0",
                Some("./dockerfiles/onomy_std.dockerfile"),
                None,
                &[],
                volumes,
                entrypoint,
                &["--entrypoint", "tmp0"],
            ),
            Container::new(
                "tmp1",
                Some("./dockerfiles/onomy_std.dockerfile"),
                None,
                &[],
                volumes,
                entrypoint,
                &["--entrypoint", "tmp1"],
            ),
        ],
        // TODO see issue on `ContainerNetwork` struct documentation
        true,
        logs_dir,
    )?;
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn tmp0() -> Result<()> {
    let host = "tmp1:26000";
    let mut nm = NetMessenger::connect(STD_TRIES, STD_DELAY, host)
        .await
        .map_add_err(|| ())?;
    let s = "hello world".to_owned();
    nm.send::<String>(&s).await?;
    Ok(())
}

async fn tmp1() -> Result<()> {
    let host = "0.0.0.0:26000";
    let mut nm = NetMessenger::listen_single_connect(host, TIMEOUT).await?;
    let s: String = nm.recv().await?;
    assert_eq!(&s, "hello world");
    Ok(())
}
