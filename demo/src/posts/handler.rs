use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::model::{CreatePost, Post, UpdatePost};
use super::repo;
use crate::error::{AppError, AppResult};
use crate::extract::{Path, ValidatedJson};
use crate::state::AppState;

/// GET /posts — 전체 목록 (최신순)
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Post>>> {
    let rows = repo::list(&state.db).await?; // sqlx::Error → AppError 자동 변환
    Ok(Json(rows.into_iter().map(Post::from).collect()))
}

/// GET /posts/{id} — 단건 조회
pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Post>> {
    repo::find(&state.db, id)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

/// POST /posts — 생성 (201 Created + Location 헤더)
pub async fn create(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<CreatePost>, // 본문 extractor는 반드시 마지막 인자
) -> AppResult<impl IntoResponse> {
    let row = repo::insert(&state.db, &body).await?;
    let location = format!("/posts/{}", row.id);

    Ok((
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(Post::from(row)),
    ))
}

/// PATCH /posts/{id} — 부분 수정
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ValidatedJson(body): ValidatedJson<UpdatePost>,
) -> AppResult<Json<Post>> {
    repo::update(&state.db, id, &body)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

/// DELETE /posts/{id} — 삭제 (204 No Content)
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if repo::delete(&state.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("post", id))
    }
}
