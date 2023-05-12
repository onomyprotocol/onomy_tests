use awint::awi::*;
use super_orchestrator::{MapAddError, Result};

pub const ONOMY_BASE: &str = "fedora:38";

/// First, this splits by `separate`, trims outer whitespace, sees if `key` is
/// prefixed, if so it also strips `inter_key_val` and returns the stripped and
/// trimmed value.
pub fn get_separated_val(
    input: &str,
    separate: &str,
    key: &str,
    inter_key_val: &str,
) -> Result<String> {
    let mut value = None;
    for line in input.split(separate) {
        if let Some(x) = line.trim().strip_prefix(key) {
            if let Some(y) = x.trim().strip_prefix(inter_key_val) {
                value = Some(y.trim().to_owned());
                break
            }
        }
    }
    value.map_add_err(|| format!("get_separated_val() -> key \"{key}\" not found"))
}

/// Given `units_of_nom` in units of NOM, returns a string of the decimal number
/// of aNOM appended with "anom"
pub fn nom(units_of_nom: f64) -> String {
    // we need 60 bits plus 54 bits for the full repr, round up to 128
    let mut f = FP::new(false, inlawi!(0u128), 0).unwrap();
    FP::f64_(&mut f, units_of_nom);
    // move fixed point to middle
    f.lshr_(64).unwrap();
    f.set_fp(f.fp() - 64);
    f.digit_cin_mul_(0, 10usize.pow(18));
    let mut s = FP::to_str_general(&f, 10, false, 1, 1, 4096).unwrap().0;
    s.push_str("anom");
    s
}

#[test]
fn test_get_separated_val() {
    let s = "address:    0x2b4e4d79e3e9dBBB170CCD78419520d1DCBb4B3f\n
    public  : 0x04b141241511b1\n    private  :=\"hello world\"    \n";
    assert_eq!(
        &get_separated_val(s, "\n", "address", ":").unwrap(),
        "0x2b4e4d79e3e9dBBB170CCD78419520d1DCBb4B3f"
    );
    assert_eq!(
        &get_separated_val(s, "\n", "public", ":").unwrap(),
        "0x04b141241511b1"
    );
    assert_eq!(
        &get_separated_val(s, "\n", "private", ":=").unwrap(),
        "\"hello world\""
    );
}

#[test]
fn test_nom() {
    assert_eq!(&nom(1.0), "1000000000000000000anom");
    assert_eq!(&nom(1.0e-18), "1anom");
    assert_eq!(&nom(1.0e18), "1000000000000000000000000000000000000anom");
    assert_eq!(&nom(std::f64::consts::TAU), "6283185307179586231anom");
}
