use axum::{Json, extract::Multipart, extract::State, http::StatusCode, response::IntoResponse};

use super::model::{CreatePost, Post, UpdatePost};
use super::repo;
use crate::auth::extractor::AuthUser;
use crate::error::{AppError, AppResult};
use crate::extract::{Path, ValidatedJson, ValidatedQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;
use crate::upload;

/// GET /posts — 페이지 목록. 인증 불필요.
///
/// `?page=1&per_page=10&q=검색어&tag=rust`
#[utoipa::path(
    get,
    path = "/posts",
    tag = "posts",
    params(PageQuery),
    responses(
        (status = 200, description = "게시글 페이지", body = Page<Post>),
        (status = 422, description = "페이지 파라미터 범위 오류"),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    ValidatedQuery(query): ValidatedQuery<PageQuery>,
) -> AppResult<Json<Page<Post>>> {
    let (rows, total) = repo::list_paged(&state.db, &query).await?;
    let items = rows.into_iter().map(Post::from).collect();

    Ok(Json(Page::new(items, &query, total)))
}

/// GET /posts/{id} — 단건 조회. 인증 불필요.
#[utoipa::path(
    get,
    path = "/posts/{id}",
    tag = "posts",
    params(("id" = i64, Path, description = "게시글 id")),
    responses(
        (status = 200, description = "게시글", body = Post),
        (status = 404, description = "없는 게시글"),
    )
)]
pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Post>> {
    repo::find(&state.db, id)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

/// POST /posts — 생성. **인증 필요** (인자에 `AuthUser`가 있다는 것이 곧 가드다)
#[utoipa::path(
    post,
    path = "/posts",
    tag = "posts",
    request_body = CreatePost,
    security(("bearer_auth" = [])),
    responses(
        (status = 201, description = "생성됨", body = Post),
        (status = 401, description = "인증 필요"),
        (status = 422, description = "검증 실패"),
    )
)]
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
#[utoipa::path(
    patch,
    path = "/posts/{id}",
    tag = "posts",
    params(("id" = i64, Path, description = "게시글 id")),
    request_body = UpdatePost,
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "수정됨", body = Post),
        (status = 403, description = "본인 글이 아님"),
        (status = 404, description = "없는 게시글"),
    )
)]
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
#[utoipa::path(
    delete,
    path = "/posts/{id}",
    tag = "posts",
    params(("id" = i64, Path, description = "게시글 id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "삭제됨"),
        (status = 403, description = "본인 글이 아님"),
        (status = 404, description = "없는 게시글"),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let row = ensure_owner(&state, id, &auth).await?;

    if repo::delete(&state.db, id).await? {
        // 글을 지웠으면 썸네일 파일도 정리한다.
        if let Some(thumbnail) = row.thumbnail {
            upload::remove_if_exists(&state.upload_dir, &thumbnail).await;
        }
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("post", id))
    }
}

/// POST /posts/{id}/thumbnail — 썸네일 업로드 (multipart/form-data). **인증 필요**
#[utoipa::path(
    post,
    path = "/posts/{id}/thumbnail",
    tag = "posts",
    params(("id" = i64, Path, description = "게시글 id")),
    request_body(content = String, description = "multipart/form-data의 `file` 필드", content_type = "multipart/form-data"),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "썸네일이 설정된 게시글", body = Post),
        (status = 400, description = "지원하지 않는 형식"),
        (status = 403, description = "본인 글이 아님"),
    )
)]
pub async fn upload_thumbnail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    multipart: Multipart, // 본문 extractor라서 마지막 인자
) -> AppResult<Json<Post>> {
    let existing = ensure_owner(&state, id, &auth).await?;

    let filename = upload::save_image(&state.upload_dir, multipart).await?;

    // 새 파일을 저장한 뒤 기존 파일을 지운다. (순서가 반대면 실패 시 둘 다 잃는다)
    if let Some(old) = existing.thumbnail {
        upload::remove_if_exists(&state.upload_dir, &old).await;
    }

    repo::set_thumbnail(&state.db, id, &filename)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

/// 글이 존재하는지 + 요청자가 작성자인지 확인하고, 그 행을 돌려준다.
///
/// - 글이 없으면 404
/// - 남의 글이면 403
///
/// **인증(authentication)과 인가(authorization)는 다른 문제**라는 걸 보여주는 자리.
/// extractor는 "누구인지"까지만 알려주고, "무엇을 할 수 있는지"는 여기서 판단한다.
async fn ensure_owner(
    state: &AppState,
    post_id: i64,
    auth: &AuthUser,
) -> AppResult<super::model::PostRow> {
    let row = repo::find(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::not_found("post", post_id))?;

    match row.author_id {
        Some(owner_id) if owner_id == auth.id => Ok(row),
        // 인증 도입 전에 만들어진 글(author_id IS NULL)은 주인이 없으므로 수정 불가로 본다.
        _ => Err(AppError::Forbidden(
            "본인이 작성한 글만 수정/삭제할 수 있습니다".into(),
        )),
    }
}
