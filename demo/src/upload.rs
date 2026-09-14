//! 파일 업로드 저장 로직.
//!
//! 업로드는 **사용자가 준 값이 파일 시스템에 닿는** 유일한 지점이라 보안이 특히 중요하다.

use std::path::{Path, PathBuf};

use axum::extract::Multipart;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

/// 허용할 이미지 MIME 타입과 확장자.
const ALLOWED: &[(&str, &str)] = &[
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];

/// 썸네일 최대 크기 (5MB).
pub const MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;

pub async fn ensure_dir(dir: &str) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    Ok(())
}

/// multipart 본문에서 첫 번째 파일 필드를 찾아 저장하고, **저장된 파일명**을 반환한다.
pub async fn save_image(dir: &str, mut multipart: Multipart) -> AppResult<String> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart 파싱 실패: {e}")))?
    {
        // 폼 필드 이름이 "file"인 것만 처리한다.
        if field.name() != Some("file") {
            continue;
        }

        let content_type = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_default();

        let extension = ALLOWED
            .iter()
            .find(|(mime, _)| *mime == content_type)
            .map(|(_, ext)| *ext)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "지원하지 않는 파일 형식입니다: {content_type} (jpeg/png/gif/webp만 가능)"
                ))
            })?;

        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("파일을 읽지 못했습니다: {e}")))?;

        if bytes.is_empty() {
            return Err(AppError::BadRequest("빈 파일입니다".into()));
        }
        if bytes.len() > MAX_UPLOAD_BYTES {
            return Err(AppError::BadRequest(format!(
                "파일이 너무 큽니다 (최대 {}MB)",
                MAX_UPLOAD_BYTES / 1024 / 1024
            )));
        }

        // ⭐ 클라이언트가 보낸 파일명을 절대 그대로 쓰지 않는다.
        //    "../../etc/passwd" 같은 값이 오면 경로 조작(path traversal) 공격이 된다.
        //    UUID로 새 이름을 만들고, 확장자는 **검증된 MIME에서** 가져온다.
        let filename = format!("{}.{extension}", uuid::Uuid::new_v4());
        let path: PathBuf = Path::new(dir).join(&filename);

        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("파일 생성 실패: {e}")))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("파일 쓰기 실패: {e}")))?;

        return Ok(filename);
    }

    Err(AppError::BadRequest(
        "`file` 이름의 파일 필드가 필요합니다".into(),
    ))
}

/// 기존 썸네일 파일을 지운다. 실패해도 요청을 실패시키지 않는다.
pub async fn remove_if_exists(dir: &str, filename: &str) {
    // 방어적으로 한 번 더: 파일명에 경로 구분자가 있으면 무시한다.
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        tracing::warn!(%filename, "의심스러운 파일명 삭제 요청을 무시했습니다");
        return;
    }

    let path = Path::new(dir).join(filename);
    if let Err(e) = tokio::fs::remove_file(&path).await {
        tracing::debug!(?path, %e, "썸네일 삭제 실패(무시)");
    }
}
