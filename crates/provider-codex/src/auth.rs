use crate::{CodexError, wire::SecretJson};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthKind {
    ManagedChatgpt,
    ApiKey,
    Unsupported,
}
/// JWT claims are routing hints, never independent identity proof.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnverifiedRoutingClaims {
    pub chatgpt_user_id: Option<String>,
    pub chatgpt_account_id: Option<String>,
    pub token_account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub is_fedramp: bool,
    /// id_token `iat`: orders two copies of the same account's tokens.
    pub issued_at: Option<i64>,
    /// access_token `exp`, when the access token is a JWT.
    pub access_expires_at: Option<i64>,
    /// Workspace and user named by the access token. The access and refresh tokens
    /// travel together, so these identify who the tokens belong to even when a
    /// refresh kept an older id_token.
    pub access_account_id: Option<String>,
    pub access_user_id: Option<String>,
}
impl UnverifiedRoutingClaims {
    /// User and workspace that own the access/refresh tokens: the access token's own
    /// claims when it carries them, else the id_token's.
    pub fn token_owner(&self) -> (Option<String>, Option<String>) {
        (
            self.access_user_id
                .clone()
                .or_else(|| self.chatgpt_user_id.clone()),
            self.access_account_id
                .clone()
                .or_else(|| self.chatgpt_account_id.clone()),
        )
    }
}
impl fmt::Debug for UnverifiedRoutingClaims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UnverifiedRoutingClaims([REDACTED])")
    }
}
#[derive(Clone)]
pub struct OpaqueAuth {
    bytes: Zeroizing<Vec<u8>>,
    kind: AuthKind,
    claims: UnverifiedRoutingClaims,
}
impl fmt::Debug for OpaqueAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpaqueAuth([REDACTED])")
    }
}
/// Lenient classification of an auth.json payload, used to watch the live file.
pub struct AuthInspection {
    pub kind: AuthKind,
    pub claims: UnverifiedRoutingClaims,
    /// The tokens, the id_token and the `tokens.account_id` label do not all name
    /// one account. Codex's `persist_tokens` merges a refresh into whatever file is
    /// current (keeping the old id_token when the response has none), so an old
    /// process still holding another account can leave its tokens here.
    pub hybrid: bool,
}
impl fmt::Debug for AuthInspection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthInspection")
            .field("kind", &self.kind)
            .field("hybrid", &self.hybrid)
            .finish_non_exhaustive()
    }
}
impl OpaqueAuth {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, CodexError> {
        let bytes = Zeroizing::new(bytes);
        let (kind, claims, hybrid) = analyze(&bytes)?;
        if hybrid {
            return Err(CodexError::IdentityMismatch);
        }
        Ok(Self {
            bytes,
            kind,
            claims,
        })
    }
    pub fn inspect(bytes: &[u8]) -> Result<AuthInspection, CodexError> {
        let (kind, claims, hybrid) = analyze(bytes)?;
        Ok(AuthInspection {
            kind,
            claims,
            hybrid,
        })
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn kind(&self) -> AuthKind {
        self.kind
    }
    pub fn routing_claims(&self) -> &UnverifiedRoutingClaims {
        &self.claims
    }
}
fn analyze(bytes: &[u8]) -> Result<(AuthKind, UnverifiedRoutingClaims, bool), CodexError> {
    {
        if bytes.len() > crate::wire::MAX_FRAME {
            return Err(CodexError::OutputLimit);
        }
        let json = SecretJson::parse(bytes)?;
        let object = json.0.as_object().ok_or(CodexError::Protocol)?;
        let nonnull = |key: &str| object.get(key).is_some_and(|v| !v.is_null());
        let mode = object.get("auth_mode").and_then(|v| v.as_str());
        let unsupported = [
            "agent_identity",
            "personal_access_token",
            "bedrock_api_key",
            "bedrock_access_keys",
        ]
        .iter()
        .any(|key| nonnull(key));
        let api = object
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());
        let mut claims = UnverifiedRoutingClaims::default();
        let mut hybrid = false;
        let kind = if unsupported
            || (nonnull("auth_mode") && mode.is_none())
            || (nonnull("OPENAI_API_KEY") && api.is_none())
        {
            AuthKind::Unsupported
        } else if api.is_some() && !nonnull("tokens") && mode.is_none_or(|m| m == "apikey") {
            AuthKind::ApiKey
        } else if api.is_none() && mode.is_none_or(|m| m == "chatgpt") {
            let tokens = object
                .get("tokens")
                .and_then(|v| v.as_object())
                .ok_or(CodexError::UnsupportedAuthMode)?;
            for key in ["access_token", "refresh_token", "id_token"] {
                if tokens
                    .get(key)
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| s.trim().is_empty())
                {
                    return Err(CodexError::UnsupportedAuthMode);
                }
            }
            let jwt = tokens["id_token"].as_str().ok_or(CodexError::Protocol)?;
            let parts: Vec<_> = jwt.split('.').collect();
            if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) || parts[1].len() > 32768 {
                return Err(CodexError::Protocol);
            }
            let payload = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(parts[1])
                    .map_err(|_| CodexError::Protocol)?,
            );
            let decoded = SecretJson::parse(&payload)?;
            let auth = &decoded.0["https://api.openai.com/auth"];
            claims.chatgpt_user_id = metadata(
                auth["chatgpt_user_id"]
                    .as_str()
                    .or_else(|| auth["user_id"].as_str()),
            )?;
            claims.chatgpt_account_id = metadata(auth["chatgpt_account_id"].as_str())?;
            if tokens
                .get("account_id")
                .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err(CodexError::Protocol);
            }
            claims.token_account_id = metadata(tokens.get("account_id").and_then(|v| v.as_str()))?;
            claims.email = metadata(
                decoded.0["email"]
                    .as_str()
                    .or_else(|| decoded.0["https://api.openai.com/profile"]["email"].as_str()),
            )?;
            claims.plan_type = metadata(auth["chatgpt_plan_type"].as_str())?;
            claims.issued_at = decoded.0["iat"].as_i64();
            if let Some(access) = tokens["access_token"].as_str().and_then(jwt_payload) {
                claims.access_expires_at = access.0["exp"].as_i64();
                let auth = &access.0["https://api.openai.com/auth"];
                claims.access_account_id = metadata(auth["chatgpt_account_id"].as_str())?;
                claims.access_user_id = metadata(
                    auth["chatgpt_user_id"]
                        .as_str()
                        .or_else(|| auth["user_id"].as_str()),
                )?;
            }
            if auth
                .get("chatgpt_account_is_fedramp")
                .is_some_and(|v| !v.is_null() && !v.is_boolean())
            {
                return Err(CodexError::Protocol);
            }
            claims.is_fedramp = auth["chatgpt_account_is_fedramp"]
                .as_bool()
                .unwrap_or(false);
            let differs = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a != b);
            hybrid = differs(&claims.chatgpt_account_id, &claims.token_account_id)
                || differs(&claims.access_account_id, &claims.token_account_id)
                || differs(&claims.access_account_id, &claims.chatgpt_account_id)
                || differs(&claims.access_user_id, &claims.chatgpt_user_id);
            AuthKind::ManagedChatgpt
        } else {
            AuthKind::Unsupported
        };
        Ok((kind, claims, hybrid))
    }
}

/// Move the rotated tokens found in `rotated` (an auth.json whose access/refresh
/// tokens belong to the account saved as `base`, possibly a hybrid file) into a copy
/// of `base`. The id_token is taken only when it names the same account; every other
/// field of `base`, including `tokens.account_id`, is kept. The result must parse
/// strictly and keep the user/workspace identity of `base`.
pub fn merge_rotated_tokens(base: &[u8], rotated: &[u8]) -> Result<Vec<u8>, CodexError> {
    let original = OpaqueAuth::parse(base.to_vec())?;
    if original.kind() != AuthKind::ManagedChatgpt {
        return Err(CodexError::UnsupportedAuthMode);
    }
    let owner = |claims: &UnverifiedRoutingClaims| {
        (
            claims.chatgpt_user_id.clone(),
            claims
                .chatgpt_account_id
                .clone()
                .or_else(|| claims.token_account_id.clone()),
        )
    };
    let base_owner = owner(original.routing_claims());
    let incoming = OpaqueAuth::inspect(rotated)?.claims;
    let (user, workspace) = incoming.token_owner();
    if user.is_none() || workspace.is_none() || (user, workspace) != base_owner {
        return Err(CodexError::IdentityMismatch);
    }
    let id_token_matches = (
        incoming.chatgpt_user_id.clone(),
        incoming.chatgpt_account_id.clone(),
    ) == base_owner;
    let mut target = SecretJson::parse(base)?;
    let source = SecretJson::parse(rotated)?;
    let tokens = source.0["tokens"]
        .as_object()
        .ok_or(CodexError::UnsupportedAuthMode)?;
    let slot = target.0["tokens"]
        .as_object_mut()
        .ok_or(CodexError::UnsupportedAuthMode)?;
    let keys: &[&str] = if id_token_matches {
        &["id_token", "access_token", "refresh_token"]
    } else {
        &["access_token", "refresh_token"]
    };
    for key in keys {
        let value = tokens
            .get(*key)
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .ok_or(CodexError::UnsupportedAuthMode)?;
        slot.insert((*key).into(), serde_json::Value::String(value.into()));
    }
    if let Some(stamp) = source.0.get("last_refresh").filter(|v| v.is_string()) {
        target.0["last_refresh"] = stamp.clone();
    }
    let bytes = serde_json::to_vec_pretty(&target.0).map_err(|_| CodexError::Protocol)?;
    let merged = OpaqueAuth::parse(bytes)?;
    if merged.kind() != AuthKind::ManagedChatgpt || owner(merged.routing_claims()) != base_owner {
        return Err(CodexError::IdentityMismatch);
    }
    Ok(merged.as_bytes().to_vec())
}
/// Best-effort payload of a JWT access token; opaque tokens yield None.
fn jwt_payload(token: &str) -> Option<SecretJson> {
    let mut parts = token.split('.');
    let (_, payload, _) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || payload.len() > 32768 {
        return None;
    }
    let bytes = Zeroizing::new(URL_SAFE_NO_PAD.decode(payload).ok()?);
    SecretJson::parse(&bytes).ok()
}
/// A hybrid file's tokens re-labelled with their own id_token workspace, so an
/// account that is not saved yet can still be kept instead of discarded.
pub fn normalize_hybrid(bytes: &[u8]) -> Result<Vec<u8>, CodexError> {
    let claims = OpaqueAuth::inspect(bytes)?.claims;
    // Possible only when the id_token names the same account as the tokens.
    if claims.token_owner()
        != (
            claims.chatgpt_user_id.clone(),
            claims.chatgpt_account_id.clone(),
        )
    {
        return Err(CodexError::IdentityMismatch);
    }
    let workspace = claims
        .chatgpt_account_id
        .ok_or(CodexError::IdentityMismatch)?;
    let mut value = SecretJson::parse(bytes)?;
    value.0["tokens"]["account_id"] = serde_json::Value::String(workspace);
    let fixed = serde_json::to_vec_pretty(&value.0).map_err(|_| CodexError::Protocol)?;
    Ok(OpaqueAuth::parse(fixed)?.as_bytes().to_vec())
}
/// auth.json payload for ChatGPT tokens imported from another switcher.
pub fn chatgpt_auth_payload(
    id_token: &str,
    access_token: &str,
    refresh_token: &str,
    account_id: Option<&str>,
    last_refresh: &str,
) -> Result<Vec<u8>, CodexError> {
    let value = serde_json::json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": id_token,
            "access_token": access_token,
            "refresh_token": refresh_token,
            "account_id": account_id,
        },
        "last_refresh": last_refresh,
    });
    let bytes =
        Zeroizing::new(serde_json::to_vec_pretty(&value).map_err(|_| CodexError::Protocol)?);
    let mut value = value;
    crate::wire::wipe(&mut value);
    Ok(OpaqueAuth::parse(bytes.to_vec())?.as_bytes().to_vec())
}
/// auth.json payload for an imported OpenAI API key (saved, never selected).
pub fn api_key_auth_payload(key: &str) -> Result<Vec<u8>, CodexError> {
    let mut value = serde_json::json!({ "OPENAI_API_KEY": key });
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| CodexError::Protocol);
    crate::wire::wipe(&mut value);
    Ok(OpaqueAuth::parse(bytes?)?.as_bytes().to_vec())
}
fn metadata(value: Option<&str>) -> Result<Option<String>, CodexError> {
    value
        .map(|s| {
            if s.is_empty() || s.len() > 512 || s.chars().any(char::is_control) {
                Err(CodexError::Protocol)
            } else {
                Ok(s.to_owned())
            }
        })
        .transpose()
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
