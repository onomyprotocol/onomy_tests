use std::{env, time::Duration};

use awint::awi::*;
use clap::Parser;
use serde_json::{json, Value};
use super_orchestrator::{
    stacked_errors::{Error, MapAddError, Result},
    std_init,
};

pub const TIMEOUT: Duration = Duration::from_secs(1000);

// the `json` macro does not support const

pub fn nom_denom() -> Value {
    json!([
        {"name": "Foo Token", "symbol": "FOO", "base": "afootoken", "display": "footoken",
        "description": "Foo token", "denom_units": [{"denom": "afootoken", "exponent": 0},
        {"denom": "footoken", "exponent": 18}]},
        {"name": "NOM", "symbol": "NOM", "base": "anom", "display": "nom",
        "description": "Nom token", "denom_units": [{"denom": "anom", "exponent": 0},
        {"denom": "nom", "exponent": 18}]}
    ])
}

pub fn native_denom() -> Value {
    json!([
        {"name": "Foo Token", "symbol": "FOO", "base": "afootoken", "display": "footoken",
        "description": "Foo token", "denom_units": [{"denom": "afootoken", "exponent": 0},
        {"denom": "footoken", "exponent": 18}]},
        {"name": "Native Token", "symbol": "NATIVE", "base": "anative", "display": "native",
        "description": "Native token", "denom_units": [{"denom": "anative", "exponent": 0},
        {"denom": "native", "exponent": 18}]}
    ])
}

/// IBC NOM denom for our Consumers
pub const ONOMY_IBC_NOM: &str =
    "ibc/5872224386C093865E42B18BDDA56BCB8CDE1E36B82B391E97697520053B0513";

pub const TEST_AMOUNT: &str =
    "57896044618658097711785492504343953926634992332820282019728792003956564819967";

/// Runs the given entrypoint
#[derive(Parser, Debug, Clone)]
#[command(about)]
pub struct Args {
    /// Gets set by `onomy_std_init`
    #[arg(long, default_value_t = String::new())]
    pub bin_name: String,
    /// If left `None`, the container runner program runs, otherwise this
    /// specifies the entry_name to run
    #[arg(long)]
    pub entry_name: Option<String>,
    /// Used by Cosmovisor for the name of the Daemon (e.x. `onomyd`)
    #[arg(long, env)]
    pub daemon_name: Option<String>,
    /// Used by Cosmovisor for the home of the Daemon (e.x. `/root/.onomy`)
    #[arg(long, env)]
    pub daemon_home: Option<String>,
    #[arg(long, env)]
    pub hermes_home: Option<String>,
    #[arg(long, env)]
    pub onomy_current_version: Option<String>,
    #[arg(long, env)]
    pub onomy_upgrade_version: Option<String>,
}

/// Calls [super_orchestrator::std_init] and returns the result of
/// [crate::Args::parse]
pub fn onomy_std_init() -> Result<Args> {
    std_init().map_add_err(|| "onomy_std_init")?;
    let mut args = Args::parse();
    args.bin_name = env::args()
        .next()
        .map_add_err(|| ())?
        .split('/')
        .last()
        .unwrap()
        .to_owned();
    Ok(args)
}

/// Given `units_of_nom` in units of NOM, returns a string of the decimal number
/// of aNOM appended with "anom"
pub fn nom(units_of_nom: f64) -> String {
    token18(units_of_nom, "anom")
}

/// Converts `units_of_nom` to an integer with its units being 1e-18, and adds
/// on `denom` as a suffix
pub fn token18(units_of_nom: f64, denom: &str) -> String {
    // we need 60 bits plus 54 bits for the full repr, round up to 128
    let mut f = FP::new(false, inlawi!(0u128), 0).unwrap();
    FP::f64_(&mut f, units_of_nom);
    // move fixed point to middle
    f.lshr_(64).unwrap();
    f.set_fp(f.fp() - 64);
    f.digit_cin_mul_(0, 10usize.pow(18));
    let mut s = FP::to_str_general(&f, 10, false, 1, 1, 4096).unwrap().0;
    s.push_str(denom);
    s
}

/// If there is a "anom" suffix it is trimmed, then we convert from units of
/// 1e-18 to 1.
pub fn anom_to_nom(val: &str) -> Result<f64> {
    let tmp = val.trim_end_matches("anom");
    let (integer, fraction) = tmp.split_at(tmp.find('.').unwrap_or(tmp.len()));
    // TODO I think it is actually the `try_to_f64` that has a rounding problem
    match ExtAwi::from_str_general(
        None,
        integer,
        fraction.get(1..).unwrap_or(""),
        -18,
        10,
        bw(192),
        128,
    ) {
        Ok(o) => {
            let mut f = FP::new(false, o, 128).unwrap();
            FP::try_to_f64(&mut f).map_add_err(|| "anom_to_nom() f64 overflow")
        }
        Err(e) => {
            // `SerdeError` can't implement Error
            Err(Error::from(format!(
                "anom_to_nom() -> when converting we got {e:?}"
            )))
        }
    }
}

pub fn yaml_str_to_json_value(yaml_input: &str) -> Result<serde_json::Value> {
    // I feel like there should be a more direct path but I can't find it
    let deserializer = serde_yaml::Deserializer::from_str(yaml_input);
    let mut json_v = vec![];
    let mut serializer = serde_json::Serializer::new(&mut json_v);
    serde_transcode::transcode(deserializer, &mut serializer).map_add_err(|| ())?;
    let json_s = String::from_utf8(json_v).map_add_err(|| ())?;
    let tmp: serde_json::Value = serde_json::from_str(&json_s).map_add_err(|| ())?;
    Ok(tmp)
}

/// Calls `.to_string().trim_matches('"').to_owned()`
pub fn json_inner(json_value: &serde_json::Value) -> String {
    json_value.to_string().trim_matches('"').to_owned()
}

/// Takes a bech32 address and replaces the prefix with a new one, correctly
/// updating the checksum
pub fn reprefix_bech32(s: &str, new_prefix: &str) -> Result<String> {
    // catch most bad cases
    if new_prefix.chars().any(|c| !c.is_ascii_alphabetic()) {
        return Err(Error::from(format!(
            "new_prefix \"{new_prefix}\" is not ascii alphabetic"
        )))
    }
    let decoded = bech32::decode(s).map_err(|e| Error::boxed(Box::new(e)))?.1;
    let encoded = bech32::encode(new_prefix, decoded, bech32::Variant::Bech32).unwrap();
    Ok(encoded)
}

#[test]
fn test_nom() {
    assert_eq!(&nom(1.0), "1000000000000000000anom");
    assert_eq!(&nom(1.0e-18), "1anom");
    assert_eq!(&nom(1.0e18), "1000000000000000000000000000000000000anom");
    assert_eq!(&nom(std::f64::consts::TAU), "6283185307179586231anom");
    assert_eq!(anom_to_nom("1000000000000000000anom").unwrap(), 1.0);
    assert_eq!(anom_to_nom("1anom").unwrap(), 9.999999999999999e-19);
    assert_eq!(anom_to_nom("1").unwrap(), 9.999999999999999e-19);
    assert_eq!(anom_to_nom("0").unwrap(), 0.0);
    assert_eq!(
        anom_to_nom("1000000000000000000000000000000000000anom").unwrap(),
        1.0e18
    );
    assert_eq!(
        anom_to_nom("6283185307179586231anom").unwrap(),
        6.283185307179585
    );
    // some methods returns a decimal even if it is always zeros
    assert_eq!(anom_to_nom("1000000000000000000.00000anom").unwrap(), 1.0);
}

#[test]
fn test_reprefix_bech32() {
    assert_eq!(
        reprefix_bech32("onomy1a69w3hfjqere4crkgyee79x2mxq0w2pfj9tu2m", "cosmos").unwrap(),
        "cosmos1a69w3hfjqere4crkgyee79x2mxq0w2pfgyl2m7".to_owned()
    );
}
