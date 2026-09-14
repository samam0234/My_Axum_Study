//! 게시글 API.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use domain::service::post as post_service;
use domain::service::post::{PostPage, PostView};

use crate::auth::AuthUser;
use crate::dto::{CreatePostRequest, ListQuery, UpdatePostRequest};
use crate::error::ApiResult;
use crate::extract::{ValidatedJson, ValidatedQuery};
use crate::state::AppState;

/// GET /api/posts?page=1&per_page=10&q=검색어
pub async fn list(
    State(state): State<AppState>,
    ValidatedQuery(query): ValidatedQuery<ListQuery>,
) -> ApiResult<Json<PostPage>> {
    let page = post_service::list(&state.db, query.page, query.per_page, query.q.as_deref()).await?;
    Ok(Json(page))
}

/// GET /api/posts/{id}
pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> ApiResult<Json<PostView>> {
    Ok(Json(post_service::find(&state.db, id).await?))
}

/// POST /api/posts — 로그인 필요
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedJson(body): ValidatedJson<CreatePostRequest>,
) -> ApiResult<impl IntoResponse> {
    let created = post_service::create(&state.db, user.id, &body.title, &body.body).await?;
    tracing::info!(author = %user.username, post_id = created.id, "새 글 작성");
    Ok((StatusCode::CREATED, Json(created)))
}

/// PATCH /api/posts/{id} — 로그인 필요 + 본인 글만
///
/// 소유권 검사가 **여기에 없다.** 서비스가 `actor_id`를 받아 직접 판단한다.
/// demo/에서는 핸들러가 `ensure_owner`를 부르는 걸 잊으면 그대로 구멍이 됐다.
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i32>,
    ValidatedJson(body): ValidatedJson<UpdatePostRequest>,
) -> ApiResult<Json<PostView>> {
    let updated = post_service::update(
        &state.db,
        id,
        user.id,
        body.title.as_deref(),
        body.body.as_deref(),
    )
    .await?;

    Ok(Json(updated))
}

/// DELETE /api/posts/{id} — 로그인 필요 + 본인 글만
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i32>,
) -> ApiResult<StatusCode> {
    post_service::delete(&state.db, id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
