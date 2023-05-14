use std::time::Duration;
pub mod cosmovisor;

use awint::awi::*;

pub const ONOMY_BASE: &str = "fedora:38";
pub const TIMEOUT: Duration = Duration::from_secs(1000);

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
fn test_nom() {
    assert_eq!(&nom(1.0), "1000000000000000000anom");
    assert_eq!(&nom(1.0e-18), "1anom");
    assert_eq!(&nom(1.0e18), "1000000000000000000000000000000000000anom");
    assert_eq!(&nom(std::f64::consts::TAU), "6283185307179586231anom");
}
