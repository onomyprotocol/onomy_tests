use clap::Parser;
use common::TIMEOUT;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    net_message::NetMessenger,
    sh, std_init, MapAddError, Result,
};
use tokio::time::sleep;

/// Runs ics_basic
#[derive(Parser, Debug)]
#[command(about)]
struct Args {
    /// If left `None`, the container runner program runs, otherwise this
    /// specifies the entrypoint to run
    #[arg(short, long)]
    entrypoint: Option<String>,
}

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
        false,
        logs_dir,
    )?;
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn tmp0() -> Result<()> {
    for addr in tokio::net::lookup_host("localhost:3000").await? {
        println!("socket address is {}", addr);
    }

    let s = "hello world".to_owned();
    let mut nm = NetMessenger::connect("marketd:26000").await?;
    nm.send::<String>(&s).await?;

    sleep(TIMEOUT).await;
    Ok(())
}

async fn tmp1() -> Result<()> {
    let mut nm = NetMessenger::listen_single_connect("onomyd:26000", TIMEOUT).await?;
    let s: String = nm.recv().await?;
    dbg!(s);

    sleep(TIMEOUT).await;
    Ok(())
}
