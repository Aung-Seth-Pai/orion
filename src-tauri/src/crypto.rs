//! Password-based encryption for exported backups.
//!
//! A backup carries every `workspace_env_vars` row verbatim — the field people
//! keep API tokens in — and the file it is written to then gets copied onto a
//! USB stick, into a sync folder, or attached to an email. Whoever holds the
//! file holds the tokens. This module is what stands between those two facts.
//!
//! Nothing here touches the database or Tauri, which is deliberate: the whole
//! format is exercisable from a unit test, including the macOS behaviour, on
//! whichever machine happens to be running the suite.
//!
//! # The envelope
//!
//! ```json
//! {
//!   "orionBackup": "encrypted",
//!   "formatVersion": 1,
//!   "kdf": { "algorithm": "argon2id", "memoryKiB": 65536, "iterations": 3,
//!            "parallelism": 1, "salt": "<base64>" },
//!   "cipher": "aes-256-gcm",
//!   "nonce": "<base64>",
//!   "ciphertext": "<base64 of ciphertext || tag>"
//! }
//! ```
//!
//! Three properties are worth stating because they are easy to get wrong:
//!
//! - **The KDF parameters travel inside the file.** The machine doing the
//!   import may be slower, faster, or a different operating system entirely. It
//!   has to rebuild the key from what is written down, never from its own idea
//!   of what half a second of work costs.
//! - **The header is authenticated.** Everything except `ciphertext` is
//!   re-serialized and passed to AES-GCM as additional data, so the salt, the
//!   nonce and the cost parameters cannot be altered without the tag failing.
//!   Without that, integrity would cover the payload and leave the instructions
//!   for decrypting it unprotected.
//! - **Nothing identifying sits in clear text.** No timestamp, no machine name,
//!   no password hint. `exported_at` lives inside `BackupPayload`, which is
//!   inside the ciphertext.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Value of the `orionBackup` field. Its presence is what tells an importer
/// that a password is needed before anything can be parsed.
const MARKER: &str = "encrypted";
const FORMAT_VERSION: u32 = 1;
const ALGORITHM: &str = "argon2id";
const CIPHER: &str = "aes-256-gcm";

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Pinned rather than measured at export time.
///
/// The brief was roughly half a second per password guess. These values were
/// chosen to land near that on ordinary desktop hardware, and then fixed:
/// timing the machine at runtime would make every export depend on whatever
/// else happened to be running, and two exports of the same data would disagree
/// for no reason the user could see. They are written into the envelope anyway,
/// so these constants can be raised later without stranding a single existing
/// backup.
const M_COST_KIB: u32 = 65_536; // 64 MiB
const T_COST: u32 = 3;
const P_COST: u32 = 1;

/// Ceilings applied to parameters read **out of a file**.
///
/// The envelope is untrusted input: it arrives from wherever the user got it.
/// Without a ceiling, `"memoryKiB": 68719476736` is a one-line
/// out-of-memory crash, which turns "I was handed a bad backup" into "the app
/// died". Generous enough that a legitimate future increase still imports.
const MAX_M_COST_KIB: u32 = 1_048_576; // 1 GiB
const MAX_T_COST: u32 = 10;
const MAX_P_COST: u32 = 4;

#[derive(Serialize, Deserialize)]
struct KdfParams {
    algorithm: String,
    #[serde(rename = "memoryKiB")]
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}

/// Everything except the ciphertext: the part that is authenticated rather than
/// encrypted.
///
/// Serialized on its own to form the AAD. Struct field order is what serde
/// writes, so the bytes are reproducible on both sides without needing a
/// canonical-JSON library.
#[derive(Serialize, Deserialize)]
struct Header {
    #[serde(rename = "orionBackup")]
    orion_backup: String,
    #[serde(rename = "formatVersion")]
    format_version: u32,
    kdf: KdfParams,
    cipher: String,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    #[serde(flatten)]
    header: Header,
    ciphertext: String,
}

/// True when `text` is an encrypted backup and therefore needs a password.
///
/// Deliberately tolerant: anything that is not recognisably our envelope is
/// treated as a plaintext backup and left to the ordinary parser, which gives a
/// better message about whatever is actually wrong with it.
pub fn is_encrypted(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            v.get("orionBackup")
                .and_then(serde_json::Value::as_str)
                .map(|m| m == MARKER)
        })
        .unwrap_or(false)
}

/// Encrypts `plaintext` under `password`, returning the envelope as JSON.
pub fn encrypt_backup(plaintext: &str, password: &str) -> Result<String, String> {
    encrypt_with(plaintext, password, M_COST_KIB, T_COST, P_COST)
}

/// The body of [`encrypt_backup`], with the cost parameters supplied.
///
/// Split out so the tests can exercise the format at a cost measured in
/// microseconds. Every byte of the envelope is produced by the same code at
/// either setting; only how long the key takes to derive differs. A suite that
/// paid the real half-second per case would be slow enough that someone would
/// eventually stop running it.
fn encrypt_with(
    plaintext: &str,
    password: &str,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> Result<String, String> {
    if password.is_empty() {
        return Err("A password is required to encrypt a backup.".into());
    }

    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut salt).map_err(|e| format!("Could not generate a salt: {e}"))?;
    getrandom::fill(&mut nonce_bytes).map_err(|e| format!("Could not generate a nonce: {e}"))?;

    let header = Header {
        orion_backup: MARKER.to_string(),
        format_version: FORMAT_VERSION,
        kdf: KdfParams {
            algorithm: ALGORITHM.to_string(),
            memory_kib,
            iterations,
            parallelism,
            salt: B64.encode(salt),
        },
        cipher: CIPHER.to_string(),
        nonce: B64.encode(nonce_bytes),
    };

    let mut key = derive_key(password, &salt, memory_kib, iterations, parallelism)?;
    let aad = serde_json::to_vec(&header)
        .map_err(|e| format!("Could not prepare the backup header: {e}"))?;

    let result = (|| {
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("Could not initialise the cipher: {e}"))?;
        let nonce = Nonce::try_from(&nonce_bytes[..])
            .map_err(|_| "Internal error: wrong nonce length".to_string())?;
        cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| "Could not encrypt the backup.".to_string())
    })();
    key.zeroize();

    let envelope = Envelope {
        header,
        ciphertext: B64.encode(result?),
    };

    serde_json::to_string_pretty(&envelope)
        .map_err(|e| format!("Could not write the backup file: {e}"))
}

/// Decrypts an envelope produced by [`encrypt_backup`], returning the original
/// backup JSON.
pub fn decrypt_backup(envelope_json: &str, password: &str) -> Result<String, String> {
    let envelope: Envelope = serde_json::from_str(envelope_json)
        .map_err(|_| "This file is not a recognisable Orion backup.".to_string())?;
    let header = &envelope.header;

    if header.orion_backup != MARKER {
        return Err("This file is not an encrypted Orion backup.".into());
    }
    if header.format_version != FORMAT_VERSION {
        return Err(format!(
            "This backup uses encryption format {} — this version of Orion understands {FORMAT_VERSION}. Update Orion and try again.",
            header.format_version
        ));
    }
    if header.cipher != CIPHER {
        return Err(format!("Unsupported cipher '{}'.", header.cipher));
    }
    if header.kdf.algorithm != ALGORITHM {
        return Err(format!(
            "Unsupported key derivation '{}'.",
            header.kdf.algorithm
        ));
    }

    let kdf = &header.kdf;
    if kdf.memory_kib > MAX_M_COST_KIB || kdf.iterations > MAX_T_COST || kdf.parallelism > MAX_P_COST
    {
        return Err(
            "This backup asks for more work than Orion is willing to do to open it. The file may be damaged or deliberately malformed."
                .into(),
        );
    }

    let salt = B64
        .decode(&kdf.salt)
        .map_err(|_| "The backup's salt is not valid base64.".to_string())?;
    let nonce_bytes = B64
        .decode(&header.nonce)
        .map_err(|_| "The backup's nonce is not valid base64.".to_string())?;
    let ciphertext = B64
        .decode(&envelope.ciphertext)
        .map_err(|_| "The backup's contents are not valid base64.".to_string())?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err("The backup's nonce is the wrong length.".into());
    }

    // Rebuilt from the parsed header rather than taken from the raw text, so a
    // reordered or reformatted file still verifies while an altered one does
    // not.
    let aad = serde_json::to_vec(header)
        .map_err(|e| format!("Could not prepare the backup header: {e}"))?;

    let mut key = derive_key(
        password,
        &salt,
        kdf.memory_kib,
        kdf.iterations,
        kdf.parallelism,
    )?;

    let result = (|| {
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("Could not initialise the cipher: {e}"))?;
        let nonce = Nonce::try_from(&nonce_bytes[..])
            .map_err(|_| "The backup's nonce is the wrong length.".to_string())?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            // AES-GCM cannot tell these two apart, and claiming otherwise would
            // send someone hunting for a typo in a password that was right all
            // along, or trusting a file that has been altered.
            .map_err(|_| {
                "Could not open this backup. The password is wrong, or the file has been changed or damaged since it was exported."
                    .to_string()
            })
    })();
    key.zeroize();

    let plaintext = result?;
    String::from_utf8(plaintext)
        .map_err(|_| "The backup decrypted to something that is not text.".to_string())
}

/// Argon2id over the password, into a 32-byte AES key.
fn derive_key(
    password: &str,
    salt: &[u8],
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> Result<[u8; KEY_LEN], String> {
    let params = Params::new(memory_kib, iterations, parallelism, Some(KEY_LEN))
        .map_err(|e| format!("Invalid key derivation parameters: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Could not derive a key from the password: {e}"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"version":1,"exported_at":"2026-10-29 10:00:00","workspaces":[],"resources":[],"automation_scripts":[],"timer_logs":[],"tasks":[],"workspace_env_vars":[{"id":"e1","workspace_id":"w1","env_key":"API_TOKEN","env_value":"sk-not-a-real-secret"}]}"#;

    /// The lowest cost Argon2 accepts, for tests that are about the format
    /// rather than about the work factor.
    fn cheap(plaintext: &str, password: &str) -> String {
        encrypt_with(plaintext, password, 8, 1, 1).unwrap()
    }

    #[test]
    fn a_backup_survives_the_round_trip() {
        // The one case that runs at the real cost, so the shipped parameters
        // are known to work end to end and not just the test ones.
        let sealed = encrypt_backup(SAMPLE, "correct horse battery staple").unwrap();
        let opened = decrypt_backup(&sealed, "correct horse battery staple").unwrap();
        assert_eq!(opened, SAMPLE);
    }

    #[test]
    fn the_secret_is_not_in_the_file() {
        // The point of the feature: the token that was in the payload must not
        // be readable in what lands on disk.
        let sealed = cheap(SAMPLE, "pw");
        assert!(!sealed.contains("sk-not-a-real-secret"));
        assert!(!sealed.contains("API_TOKEN"));
    }

    #[test]
    fn a_password_of_any_language_works() {
        // The password is bytes to Argon2, so a non-ASCII one must round-trip
        // rather than being mangled by some assumption about length or case.
        for password in ["パスワード", "mot de passe é", "密码🔐", " leading space"] {
            let sealed = cheap(SAMPLE, password);
            assert_eq!(decrypt_backup(&sealed, password).unwrap(), SAMPLE);
        }
    }

    #[test]
    fn the_wrong_password_is_refused_without_guessing_why() {
        let sealed = cheap(SAMPLE, "right");
        let err = decrypt_backup(&sealed, "wrong").unwrap_err();
        assert!(err.contains("password is wrong"), "{err}");
        assert!(err.contains("changed or damaged"), "{err}");
    }

    #[test]
    fn altering_the_ciphertext_is_detected() {
        let sealed = cheap(SAMPLE, "pw");
        let mut envelope: serde_json::Value = serde_json::from_str(&sealed).unwrap();
        let mut raw = B64
            .decode(envelope["ciphertext"].as_str().unwrap())
            .unwrap();
        raw[0] ^= 0x01;
        envelope["ciphertext"] = serde_json::json!(B64.encode(raw));

        decrypt_backup(&serde_json::to_string(&envelope).unwrap(), "pw")
            .expect_err("a flipped bit must not decrypt");
    }

    #[test]
    fn altering_the_header_is_detected() {
        // The header is authenticated, not encrypted, so this is the case that
        // proves the AAD binding is actually wired up: the parameters are
        // readable but not changeable.
        let sealed = cheap(SAMPLE, "pw");
        let mut envelope: serde_json::Value = serde_json::from_str(&sealed).unwrap();
        envelope["kdf"]["iterations"] = serde_json::json!(T_COST + 1);

        let err = decrypt_backup(&serde_json::to_string(&envelope).unwrap(), "pw").unwrap_err();
        assert!(err.contains("changed or damaged"), "{err}");
    }

    #[test]
    fn reformatting_the_file_is_not_treated_as_tampering() {
        // Somebody runs the file through a JSON formatter, or a sync tool
        // rewrites it with sorted keys. The bytes differ; the meaning does not.
        let sealed = cheap(SAMPLE, "pw");
        let value: serde_json::Value = serde_json::from_str(&sealed).unwrap();
        let compact = serde_json::to_string(&value).unwrap();
        assert_eq!(decrypt_backup(&compact, "pw").unwrap(), SAMPLE);
    }

    #[test]
    fn absurd_parameters_are_refused_rather_than_attempted() {
        let sealed = cheap(SAMPLE, "pw");
        for (field, value) in [
            ("memoryKiB", serde_json::json!(MAX_M_COST_KIB + 1)),
            ("iterations", serde_json::json!(MAX_T_COST + 1)),
            ("parallelism", serde_json::json!(MAX_P_COST + 1)),
        ] {
            let mut envelope: serde_json::Value = serde_json::from_str(&sealed).unwrap();
            envelope["kdf"][field] = value;
            let err =
                decrypt_backup(&serde_json::to_string(&envelope).unwrap(), "pw").unwrap_err();
            assert!(
                err.contains("more work than Orion is willing to do"),
                "{field}: {err}"
            );
        }
    }

    #[test]
    fn an_encrypted_file_is_recognisable_without_the_password() {
        assert!(is_encrypted(&cheap(SAMPLE, "pw")));
        assert!(!is_encrypted(SAMPLE));
        assert!(!is_encrypted("not json at all"));
        assert!(!is_encrypted(""));
    }

    #[test]
    fn an_empty_password_is_not_silently_accepted() {
        // Otherwise a UI bug that passed through a blank field would produce a
        // file that looks encrypted and is openable by anyone.
        assert!(encrypt_backup(SAMPLE, "").is_err());
    }

    #[test]
    fn the_pinned_cost_is_the_one_the_app_uses() {
        // encrypt_with exists for the tests; this is what stops it drifting
        // away from what encrypt_backup actually ships.
        let sealed = encrypt_backup(SAMPLE, "pw").unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&sealed).unwrap();
        assert_eq!(envelope["kdf"]["memoryKiB"], M_COST_KIB);
        assert_eq!(envelope["kdf"]["iterations"], T_COST);
        assert_eq!(envelope["kdf"]["parallelism"], P_COST);
    }

    #[test]
    fn every_export_differs_even_for_identical_data() {
        // Fresh salt and nonce each time. If these ever matched, two exports
        // would leak that the data was unchanged, and a repeated nonce under
        // one key would break GCM outright.
        let a: serde_json::Value = serde_json::from_str(&cheap(SAMPLE, "pw")).unwrap();
        let b: serde_json::Value = serde_json::from_str(&cheap(SAMPLE, "pw")).unwrap();
        assert_ne!(a["kdf"]["salt"], b["kdf"]["salt"]);
        assert_ne!(a["nonce"], b["nonce"]);
        assert_ne!(a["ciphertext"], b["ciphertext"]);
    }

    #[test]
    fn the_envelope_says_what_it_is() {
        // A file that does not name its own algorithms cannot be opened by a
        // future version that supports more than one.
        let envelope: serde_json::Value = serde_json::from_str(&cheap(SAMPLE, "pw")).unwrap();
        assert_eq!(envelope["orionBackup"], MARKER);
        assert_eq!(envelope["formatVersion"], FORMAT_VERSION);
        assert_eq!(envelope["kdf"]["algorithm"], ALGORITHM);
        assert_eq!(envelope["cipher"], CIPHER);
    }

    /// A file encrypted by an earlier build, committed as a fixture.
    ///
    /// This is the cross-platform and cross-version guarantee in test form. It
    /// runs on both CI runners, so a change to the field names, the AAD
    /// construction or the base64 alphabet fails the build here rather than in
    /// somebody's restore six months from now.
    #[test]
    fn a_backup_from_an_earlier_build_still_opens() {
        const FIXTURE: &str = include_str!("../tests/fixtures/encrypted_backup.json");
        let opened = decrypt_backup(FIXTURE, "orion-test-password").unwrap();
        let payload: serde_json::Value = serde_json::from_str(&opened).unwrap();
        assert_eq!(payload["version"], 1);
        assert_eq!(payload["workspace_env_vars"][0]["env_key"], "API_TOKEN");
    }

    /// Writes the fixture the test above reads. Run by hand, never in CI:
    ///
    /// ```text
    /// cargo test --lib regenerate_the_golden_fixture -- --ignored
    /// ```
    ///
    /// It exists so the fixture is reproducible rather than a blob nobody
    /// remembers making. Note that regenerating it *weakens* the test it
    /// feeds: a file written by today's code proves nothing about opening a
    /// file written by yesterday's. When `FORMAT_VERSION` changes, the right
    /// move is almost always to leave this fixture alone and add a second one
    /// beside it, so both versions are covered.
    #[test]
    #[ignore = "writes into the source tree; run by hand to regenerate the fixture"]
    fn regenerate_the_golden_fixture() {
        // Shaped like a real backup, with the field encryption exists for.
        // Nothing here is a real credential or identifies a real machine.
        let payload = serde_json::json!({
            "version": 1,
            "exported_at": "2026-01-01 00:00:00",
            "workspaces": [],
            "resources": [],
            "automation_scripts": [],
            "timer_logs": [],
            "tasks": [],
            "workspace_env_vars": [{
                "workspace_id": "00000000-0000-0000-0000-000000000001",
                "env_key": "API_TOKEN",
                "env_value": "example-value-not-a-credential",
            }],
        });

        // Written at the pinned cost, so the fixture is a file the shipped app
        // could actually have produced.
        let envelope = encrypt_backup(&payload.to_string(), "orion-test-password").unwrap();
        std::fs::write("tests/fixtures/encrypted_backup.json", envelope).unwrap();
    }
}
