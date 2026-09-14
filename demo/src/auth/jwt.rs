//! JWT 발급과 검증.

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// 토큰 유효 시간(초). 학습용이라 짧게 잡았다.
pub const TOKEN_TTL_SECONDS: i64 = 60 * 60; // 1시간

/// JWT payload(클레임).
///
/// `sub`, `exp`, `iat`는 JWT 표준(RFC 7519)에 정의된 이름이다.
/// 표준 이름을 쓰면 라이브러리가 만료 검사 같은 걸 자동으로 해준다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// subject — 사용자 id
    pub sub: i64,
    /// 사용자 이름 (커스텀 클레임)
    pub username: String,
    /// issued at — 발급 시각 (Unix epoch 초)
    pub iat: i64,
    /// expiration — 만료 시각 (Unix epoch 초)
    pub exp: i64,
}

pub fn encode_token(secret: &str, user_id: i64, username: &str) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + TOKEN_TTL_SECONDS,
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(token)
}

/// 서명과 만료를 검증하고 클레임을 돌려준다.
pub fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    // Validation::default()는 exp(만료) 검사를 자동으로 포함한다.
    let validation = Validation::new(Algorithm::HS256);

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
}
