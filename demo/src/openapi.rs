//! OpenAPI 명세 정의.
//!
//! FastAPI는 타입만 보고 문서를 자동 생성하지만, Rust에는 런타임 리플렉션이 없어서
//! **`#[utoipa::path(...)]` 어트리뷰트로 명시적으로 적어 준다.**
//! 손이 더 가는 대신, 문서에 적는 내용(상태 코드·설명·보안 요구사항)을 정확히 통제할 수 있다.

use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Axum 학습용 게시판 API",
        version = "0.1.0",
        description = "study1~10에서 단계적으로 만든 게시판 + 인증 API",
    ),
    paths(
        crate::auth::handler::register,
        crate::auth::handler::login,
        crate::auth::handler::me,
        crate::posts::handler::list,
        crate::posts::handler::get_one,
        crate::posts::handler::create,
        crate::posts::handler::update,
        crate::posts::handler::delete,
        crate::posts::handler::upload_thumbnail,
    ),
    components(schemas(
        crate::posts::model::Post,
        crate::posts::model::CreatePost,
        crate::posts::model::UpdatePost,
        crate::pagination::Page<crate::posts::model::Post>,
        crate::auth::model::UserPublic,
        crate::auth::model::RegisterRequest,
        crate::auth::model::LoginRequest,
        crate::auth::model::TokenResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "회원가입 / 로그인 / 내 정보"),
        (name = "posts", description = "게시판 CRUD"),
    )
)]
pub struct ApiDoc;

/// Swagger UI의 "Authorize" 버튼을 만들어 주는 보안 스킴 정의.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}
