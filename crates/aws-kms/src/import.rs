//! Imported key material: `GetParametersForImport`, `ImportKeyMaterial`, `DeleteImportedKeyMaterial`. A key made with origin
//! `EXTERNAL` has no material and is `PendingImport`. The caller asks for a wrapping public key, wraps 32 bytes of its own with it
//! (RSA-OAEP, or RSA-AES key wrap: the AES key RSA-OAEP-wrapped, then the material wrapped by AES-KWP), and imports them. Only
//! symmetric encryption keys can import. The words of the refusals are not recorded.

use aes_kw::KeyInit;
use aes_kw::KwpAes256;
use aws_model_kms::keys::{
    DeleteImportedKeyMaterialRequest as Delete, DeleteImportedKeyMaterialResponse as Deleted,
    GetParametersForImportRequest as Params, GetParametersForImportResponse as ParamsMade,
    ImportKeyMaterialRequest as Import_, ImportKeyMaterialResponse as Imported,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::crypto::material_id;
use crate::find::not_found_key;
use crate::keys::{invalid_state, no_randomness, seconds};
use crate::model::{DAY_MS, Import, Key, Material, State};
use crate::rsa_keys;

const ALGORITHMS: [&str; 4] = [
    "RSAES_OAEP_SHA_1",
    "RSAES_OAEP_SHA_256",
    "RSA_AES_KEY_WRAP_SHA_1",
    "RSA_AES_KEY_WRAP_SHA_256",
];
const KEY_BYTES: usize = 32;

fn bad(code: &'static str, message: &str) -> ApiError {
    ApiError::modeled(code, message)
}

/// Opens what a caller wrapped: the 32 bytes of material.
fn unwrap_material(algorithm: &str, wrapping: &[u8], wrapped: &[u8]) -> Option<Vec<u8>> {
    let oaep = if algorithm.ends_with("SHA_1") {
        "RSAES_OAEP_SHA_1"
    } else {
        "RSAES_OAEP_SHA_256"
    };
    if !algorithm.starts_with("RSA_AES_KEY_WRAP") {
        return rsa_keys::decrypt(oaep, wrapping, wrapped);
    }
    let (rsa_part, kwp_part) = wrapped.split_at_checked(rsa_keys::modulus_len(wrapping)?)?;
    let aes_key = rsa_keys::decrypt(oaep, wrapping, rsa_part)?;
    let kwp = KwpAes256::new_from_slice(&aes_key).ok()?;
    let mut buf = vec![0_u8; kwp_part.len()];
    kwp.unwrap_key(kwp_part, &mut buf).ok().map(<[u8]>::to_vec)
}

impl Core {
    pub(crate) fn get_parameters_for_import(&mut self, call: &Call) -> Reply {
        let v = request::<Params>(call)?;
        let algorithm = v.get(Params::WRAPPING_ALGORITHM).unwrap_or_default();
        if !ALGORITHMS.contains(&algorithm) {
            return Err(validation(format!(
                "The wrapping algorithm {algorithm} is not built in doze-aws."
            )));
        }
        let bits = match v.get(Params::WRAPPING_KEY_SPEC).unwrap_or_default() {
            "RSA_2048" => 2048,
            "RSA_3072" => 3072,
            "RSA_4096" => 4096,
            other => {
                return Err(validation(format!(
                    "The wrapping key spec {other} is not valid."
                )));
            }
        };
        let id = self.locate(call, v.get(Params::KEY_ID).unwrap_or_default())?;
        let now = self.now_ms();
        let key = self.keys.get(&id).ok_or_else(|| not_found_key(call, &id))?;
        external(key)?;
        let der = rsa_keys::generate(bits).ok_or_else(no_randomness)?;
        let public = rsa_keys::public_key(&der).ok_or_else(no_randomness)?;
        let token = aws_seal::random_bytes::<48>()
            .ok_or_else(no_randomness)?
            .to_vec();
        let valid_to_ms = now + DAY_MS;
        let wrapping = self.sealer.seal(&der).ok_or_else(no_randomness)?;
        let arn = key.arn.clone();
        if let Some(k) = self.keys.get_mut(&id) {
            k.import = Some(Import {
                token: token.clone(),
                wrapping,
                algorithm: algorithm.to_owned(),
                valid_to_ms,
            });
        }
        reply::<ParamsMade>(|o| {
            o.put(ParamsMade::KEY_ID, &arn);
            o.put_blob(ParamsMade::IMPORT_TOKEN, &token);
            o.put_blob(ParamsMade::PUBLIC_KEY, &public);
            o.put_time(ParamsMade::PARAMETERS_VALID_TO, seconds(valid_to_ms));
        })
    }

    pub(crate) fn import_key_material(&mut self, call: &Call) -> Reply {
        let v = request::<Import_>(call)?;
        let id = self.locate(call, v.get(Import_::KEY_ID).unwrap_or_default())?;
        let now = self.now_ms();
        let key = self.keys.get(&id).ok_or_else(|| not_found_key(call, &id))?;
        external(key)?;
        let params = key.import.as_ref().ok_or_else(|| {
            bad(
                "InvalidImportTokenException",
                "Call GetParametersForImport first: there is no import token for this key.",
            )
        })?;
        if v.get(Import_::IMPORT_TOKEN) != Some(params.token.as_slice()) {
            return Err(bad(
                "InvalidImportTokenException",
                "The import token is not the one this key was given.",
            ));
        }
        if params.valid_to_ms <= now {
            return Err(bad(
                "ExpiredImportTokenException",
                "The import token has expired: call GetParametersForImport again.",
            ));
        }
        let wrapping = self
            .sealer
            .open(&params.wrapping)
            .ok_or_else(|| ApiError::internal("the wrapping key could not be opened"))?;
        let raw = unwrap_material(&params.algorithm, &wrapping, v.get(Import_::ENCRYPTED_KEY_MATERIAL).unwrap_or_default())
            .filter(|m| m.len() == KEY_BYTES)
            .ok_or_else(|| bad("IncorrectKeyMaterialException", "The key material is not the 256 bits of an AES key, or was not wrapped with the key this import gave."))?;
        let (model, valid_to) = expiry(
            v.get(Import_::EXPIRATION_MODEL),
            v.get(Import_::VALID_TO),
            now,
        )?;
        let material_hex = aws_core::hex::encode(&material_id(&raw));
        if key.current().is_some_and(|m| m.id != material_hex) {
            return Err(bad(
                "IncorrectKeyMaterialException",
                "The key material is not the material this key had before.",
            ));
        }
        let sealed = self.sealer.seal(&raw).ok_or_else(no_randomness)?;
        let arn = key.arn.clone();
        if let Some(k) = self.keys.get_mut(&id) {
            if k.materials.is_empty() {
                k.materials.push(Material {
                    id: material_hex.clone(),
                    sealed,
                    created_ms: now,
                    retired_by: None,
                    retired_ms: None,
                });
            }
            k.import = None;
            k.expiration_model = Some(model);
            k.valid_to_ms = valid_to;
            if k.state == State::PendingImport {
                k.state = State::Enabled;
            }
        }
        if let Some(at) = valid_to {
            self.purge_at(at);
        }
        reply::<Imported>(|o| {
            o.put(Imported::KEY_ID, &arn);
            o.put(Imported::KEY_MATERIAL_ID, &material_hex);
        })
    }

    pub(crate) fn delete_imported_key_material(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let id = self.locate(call, v.get(Delete::KEY_ID).unwrap_or_default())?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        external(key)?;
        let gone = key
            .materials
            .pop()
            .ok_or_else(|| invalid_state(format!("{} has no imported key material.", key.arn)))?;
        key.state = State::PendingImport;
        key.valid_to_ms = None;
        key.expiration_model = None;
        let arn = key.arn.clone();
        reply::<Deleted>(|o| {
            o.put(Deleted::KEY_ID, &arn);
            o.put(Deleted::KEY_MATERIAL_ID, &gone.id);
        })
    }

    /// Takes the material away from keys whose imported material has expired: they wait to be imported again.
    pub(crate) fn expire_imports(&mut self, now: i64) {
        let due: Vec<String> = self
            .keys
            .iter()
            .filter(|(_, k)| k.valid_to_ms.is_some_and(|at| at <= now))
            .map(|(id, _)| id.to_owned())
            .collect();
        for id in due {
            if let Some(k) = self.keys.get_mut(&id) {
                k.materials.clear();
                k.valid_to_ms = None;
                if k.state != State::PendingDeletion {
                    k.state = State::PendingImport;
                }
            }
        }
    }
}

fn external(key: &Key) -> Result<(), ApiError> {
    if !key.external {
        return Err(ApiError::modeled(
            "UnsupportedOperationException",
            format!(
                "{} origin is AWS_KMS: only an EXTERNAL key imports material.",
                key.arn
            ),
        ));
    }
    if key.state == State::PendingDeletion {
        return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
    }
    Ok(())
}

/// The expiration model and time of an import.
fn expiry(
    model: Option<&str>,
    valid_to: Option<f64>,
    now: i64,
) -> Result<(String, Option<i64>), ApiError> {
    let model = model.unwrap_or(if valid_to.is_some() {
        "KEY_MATERIAL_EXPIRES"
    } else {
        "KEY_MATERIAL_DOES_NOT_EXPIRE"
    });
    match (model, valid_to) {
        ("KEY_MATERIAL_DOES_NOT_EXPIRE", None) => Ok((model.to_owned(), None)),
        ("KEY_MATERIAL_DOES_NOT_EXPIRE", Some(_)) => Err(validation(
            "ValidTo can't be given when the key material does not expire.",
        )),
        ("KEY_MATERIAL_EXPIRES", Some(at)) => {
            #[allow(clippy::cast_possible_truncation)]
            let at_ms = (at * 1000.0) as i64;
            if at_ms <= now {
                return Err(validation("ValidTo must be in the future."));
            }
            Ok((model.to_owned(), Some(at_ms)))
        }
        _ => Err(validation("KEY_MATERIAL_EXPIRES needs a ValidTo.")),
    }
}
