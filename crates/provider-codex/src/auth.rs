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
impl OpaqueAuth {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, CodexError> {
        let bytes = Zeroizing::new(bytes);
        if bytes.len() > crate::wire::MAX_FRAME {
            return Err(CodexError::OutputLimit);
        }
        let json = SecretJson::parse(&bytes)?;
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
            if auth
                .get("chatgpt_account_is_fedramp")
                .is_some_and(|v| !v.is_null() && !v.is_boolean())
            {
                return Err(CodexError::Protocol);
            }
            claims.is_fedramp = auth["chatgpt_account_is_fedramp"]
                .as_bool()
                .unwrap_or(false);
            if let (Some(a), Some(b)) = (&claims.chatgpt_account_id, &claims.token_account_id)
                && a != b
            {
                return Err(CodexError::IdentityMismatch);
            }
            AuthKind::ManagedChatgpt
        } else {
            AuthKind::Unsupported
        };
        Ok(Self {
            bytes,
            kind,
            claims,
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
