//! 비밀번호 해싱. demo/ study6과 같은 내용이지만, 도메인 크레이트에 산다.

use anyhow::Context;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let hash: PasswordHash = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(|e| anyhow::anyhow!("비밀번호 해싱 실패: {e}"))?;
        Ok(hash.to_string())
    })
    .await
    .context("해싱 태스크가 패닉했습니다")?
}

pub async fn verify(password: String, password_hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|e| anyhow::anyhow!("저장된 해시 형식이 잘못되었습니다: {e}"))?;

        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .context("검증 태스크가 패닉했습니다")?
}
