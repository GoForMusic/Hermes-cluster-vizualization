use super::*;

#[test]
fn reads_the_forms_the_api_uses() {
    for (text, want) in [
        ("2", 2.0),
        ("1.5", 1.5),
        ("100m", 0.1),
        ("250u", 0.00025),
        ("500n", 5e-7),
        ("1k", 1e3),
        ("3M", 3e6),
        ("4G", 4e9),
        ("1Ki", 1024.0),
        ("512Mi", 536_870_912.0),
        ("2Gi", 2_147_483_648.0),
        ("1Ti", 1_099_511_627_776.0),
        ("129e6", 129e6),
        ("12E3", 12e3),
        ("1E", 1e18),
    ] {
        let got = parse(text).unwrap_or_else(|| panic!("{text}"));
        assert!(
            (got - want).abs() <= want.abs() * 1e-12,
            "{text}: {got} != {want}"
        );
    }
}

#[test]
fn refuses_what_is_not_a_quantity() {
    for bad in ["", "abc", "12x", "Gi", "1..2", "e5"] {
        assert_eq!(parse(bad), None, "{bad:?}");
    }
}

#[test]
fn cpu_in_millicores() {
    assert_eq!(
        (milli("100m"), milli("2"), milli("1500u")),
        (Some(100.0), Some(2000.0), Some(2.0))
    );
}
