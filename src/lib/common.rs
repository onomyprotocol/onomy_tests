use std::time::Duration;
pub mod cosmovisor;
use awint::awi::*;
use clap::Parser;
use super_orchestrator::{Error, MapAddError, Result};

pub const ONOMY_BASE: &str = "fedora:38";
pub const TIMEOUT: Duration = Duration::from_secs(1000);

/// Runs the given entrypoint
#[derive(Parser, Debug)]
#[command(about)]
pub struct Args {
    /// If left `None`, the container runner program runs, otherwise this
    /// specifies the entrypoint to run
    #[arg(short, long)]
    pub entrypoint: Option<String>,
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
