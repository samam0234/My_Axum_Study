use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::model::{CreatePost, Post, UpdatePost};
use super::repo;
use crate::auth::extractor::AuthUser;
use crate::error::{AppError, AppResult};
use crate::extract::{Path, ValidatedJson};
use crate::state::AppState;

/// GET /posts — 전체 목록 (최신순). 인증 불필요.
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Post>>> {
    let rows = repo::list(&state.db).await?; // sqlx::Error → AppError 자동 변환
    Ok(Json(rows.into_iter().map(Post::from).collect()))
}

/// GET /posts/{id} — 단건 조회. 인증 불필요.
pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Post>> {
    repo::find(&state.db, id)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

/// POST /posts — 생성. **인증 필요** (인자에 `AuthUser`가 있다는 것이 곧 가드다)
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,                                 // ← 토큰이 없거나 틀리면 여기서 401
    ValidatedJson(body): ValidatedJson<CreatePost>, // 본문 extractor는 반드시 마지막 인자
) -> AppResult<impl IntoResponse> {
    // 작성자는 클라이언트가 보낸 값이 아니라 토큰에서 나온 값이다.
    let row = repo::insert(&state.db, &body, &auth.username, auth.id).await?;
    let location = format!("/posts/{}", row.id);

    Ok((
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(Post::from(row)),
    ))
}

/// PATCH /posts/{id} — 부분 수정. **인증 필요**
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    ValidatedJson(body): ValidatedJson<UpdatePost>,
) -> AppResult<Json<Post>> {
    // 인가(authorization): 인증만으로는 부족하다. 남의 글을 고칠 수 있으면 안 된다.
    ensure_owner(&state, id, &auth).await?;

    repo::update(&state.db, id, &body)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

/// DELETE /posts/{id} — 삭제. **인증 필요**
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    ensure_owner(&state, id, &auth).await?;

    if repo::delete(&state.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("post", id))
    }
}

/// 글이 존재하는지 + 요청자가 작성자인지 확인한다.
///
/// - 글이 없으면 404
/// - 남의 글이면 403
///
/// **인증(authentication)과 인가(authorization)는 다른 문제**라는 걸 보여주는 자리.
/// extractor는 "누구인지"까지만 알려주고, "무엇을 할 수 있는지"는 여기서 판단한다.
async fn ensure_owner(state: &AppState, post_id: i64, auth: &AuthUser) -> AppResult<()> {
    let row = repo::find(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::not_found("post", post_id))?;

    match row.author_id {
        Some(owner_id) if owner_id == auth.id => Ok(()),
        // 인증 도입 전에 만들어진 글(author_id IS NULL)은 주인이 없으므로 수정 불가로 본다.
        _ => Err(AppError::Forbidden("본인이 작성한 글만 수정/삭제할 수 있습니다".into())),
    }
}
