use common::container_runner;
use log::info;
use onomy_test_lib::{
    onomy_std_init,
    super_orchestrator::{
        net_message::NetMessenger,
        stacked_errors::{MapAddError, Result},
        STD_DELAY, STD_TRIES,
    },
    TIMEOUT,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    dbg!(&args);

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "tmp0" => tmp0().await,
            "tmp1" => tmp1().await,
            _ => format!("entry_name \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        container_runner(&args, &[("onomy_std", "tmp0"), ("onomy_std", "tmp1")]).await
    }
}

async fn tmp0() -> Result<()> {
    let host = "tmp1:26000";
    info!("connecting");
    let mut nm = NetMessenger::connect(STD_TRIES, STD_DELAY, host)
        .await
        .map_add_err(|| ())?;
    let s = "hello world".to_owned();
    info!("sending \"{s}\"");
    nm.send::<String>(&s).await?;
    Ok(())
}

async fn tmp1() -> Result<()> {
    let host = "0.0.0.0:26000";
    let mut nm = NetMessenger::listen_single_connect(host, TIMEOUT).await?;
    let s: String = nm.recv().await?;
    info!("waiting to recieve");
    assert_eq!(&s, "hello world");
    Ok(())
}
