use super::*;

#[test]
fn a_secret_round_trips_through_encryption() {
    let key = Key::random();
    let enc = encrypt(&key, "hunter2-kubeconfig");
    assert!(enc.starts_with("enc1:"), "{enc}");
    assert_eq!(decrypt(&key, &enc).unwrap(), "hunter2-kubeconfig");
}

#[test]
fn an_empty_secret_stays_empty() {
    let key = Key::random();
    let enc = encrypt(&key, "");
    assert_eq!(enc, "");
    assert_eq!(decrypt(&key, &enc).unwrap(), "");
}

#[test]
fn two_encryptions_of_the_same_value_differ() {
    // Different nonces: a leaked hub.db never lets one guess which sources share a secret.
    let key = Key::random();
    assert_ne!(encrypt(&key, "same-token"), encrypt(&key, "same-token"));
}

#[test]
fn the_wrong_key_does_not_decrypt() {
    let enc = encrypt(&Key::random(), "hunter2");
    assert!(decrypt(&Key::random(), &enc).is_err());
}

#[test]
fn a_tampered_ciphertext_does_not_decrypt() {
    let key = Key::random();
    let mut enc = encrypt(&key, "hunter2").into_bytes();
    let last = enc.len() - 1;
    enc[last] ^= 1; // flip a bit in the base64 tail (ciphertext or tag)
    assert!(decrypt(&key, &String::from_utf8(enc).unwrap()).is_err());
}

#[test]
fn a_hex_key_must_be_exactly_32_bytes() {
    assert!(
        Key::from_hex(&"ab".repeat(32)).is_ok(),
        "64 hex chars decode fine"
    );
    assert!(Key::from_hex(&"ab".repeat(16)).is_err(), "too short");
    assert!(Key::from_hex("not hex").is_err());
}

#[test]
fn plaintext_is_told_apart_from_encrypted() {
    assert!(is_plaintext("some-old-kubeconfig"));
    assert!(!is_plaintext(""));
    assert!(!is_plaintext(&encrypt(&Key::random(), "x")));
}
