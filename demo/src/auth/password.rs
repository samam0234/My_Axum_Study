//! 비밀번호 해싱.
//!
//! Argon2는 **일부러 느리게** 설계된 알고리즘이다(2015 Password Hashing Competition 우승).
//! 느리다는 건 곧 **async 워커 스레드를 수십 ms씩 붙잡는다**는 뜻이라,
//! 반드시 `spawn_blocking`으로 별도 스레드 풀에 넘겨야 한다.

use anyhow::Context;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

/// 평문 비밀번호 → PHC 문자열
/// (`$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>` 형태로 알고리즘·파라미터·솔트가 함께 들어 있다)
pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        // argon2 0.6의 hash_password는 솔트를 알아서 무작위로 만들어 준다.
        // 같은 비밀번호라도 사용자마다 해시가 달라진다.
        let hash: PasswordHash = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(|e| anyhow::anyhow!("비밀번호 해싱 실패: {e}"))?;

        Ok(hash.to_string())
    })
    .await
    .context("해싱 태스크가 패닉했습니다")?
}

/// 평문 비밀번호가 저장된 해시와 일치하는지 확인
pub async fn verify(password: String, password_hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|e| anyhow::anyhow!("저장된 해시 형식이 잘못되었습니다: {e}"))?;

        // 검증 실패(비밀번호 불일치)는 "에러"가 아니라 false로 다룬다.
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .context("검증 태스크가 패닉했습니다")?
}
