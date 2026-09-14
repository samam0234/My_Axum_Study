//! JWT 발급/검증과 인증 extractor.
//!
//! demo/ study6과 같은 개념이지만, 토큰을 **헤더가 아니라 쿠키**로도 받는다.
//! 브라우저에서 직접 쓰는 웹사이트라서 localStorage보다 HttpOnly 쿠키가 안전하다.

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
    response::Response,
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::error;
use crate::state::AppState;

pub const COOKIE_NAME: &str = "session";
pub const TOKEN_TTL_SECONDS: i64 = 60 * 60 * 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i32,
    pub username: String,
    pub iat: i64,
    pub exp: i64,
}

pub fn encode_token(secret: &str, user_id: i32, username: &str) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + TOKEN_TTL_SECONDS,
    };

    Ok(encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|data| data.claims)
}

/// 로그인 상태를 담는 쿠키를 만든다.
///
/// - `HttpOnly`: JavaScript에서 읽을 수 없다 → XSS로 토큰을 훔치기 어렵다
/// - `SameSite=Lax`: 다른 사이트에서 시작된 요청에 쿠키가 안 붙는다 → CSRF 완화
/// - `Secure`: HTTPS에서만 전송. 로컬 개발(http)에서는 끈다
pub fn session_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={TOKEN_TTL_SECONDS}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

/// 인증된 사용자. 핸들러 인자에 적는 것만으로 라우트가 보호된다.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i32,
    pub username: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // 1) Authorization: Bearer <토큰>  또는  2) session 쿠키
        let token = bearer_token(parts).or_else(|| cookie_token(parts));

        let Some(token) = token else {
            return Err(error::unauthorized("로그인이 필요합니다"));
        };

        let claims = decode_token(&state.jwt_secret, &token).map_err(|e| {
            use jsonwebtoken::errors::ErrorKind;
            match e.kind() {
                ErrorKind::ExpiredSignature => error::unauthorized("세션이 만료되었습니다"),
                _ => error::unauthorized("유효하지 않은 인증 정보입니다"),
            }
        })?;

        Ok(AuthUser {
            id: claims.sub,
            username: claims.username,
        })
    }
}

fn bearer_token(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_string)
}

fn cookie_token(parts: &Parts) -> Option<String> {
    let raw = parts.headers.get(header::COOKIE)?.to_str().ok()?;

    raw.split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_string())
}
