# Axum 학습 저장소

Rust 웹 프레임워크 [Axum](https://github.com/tokio-rs/axum)을 기초 개념부터 단계별로 실습하며 정리하는 개인 학습 저장소입니다.
같은 워크스페이스의 `Spring_Boot/`, `python/FastApi/`, `nest.js/`와 **동일한 커리큘럼(게시판 API + 인증)** 을 Rust 방식으로 다시 구현하면서, GC 없는 언어에서 웹 서버를 짜면 무엇이 달라지는지를 비교하는 것이 목표입니다.

- `studyN.md` 문서에 개념 정리와 실습 과정, 실제로 실행해서 확인한 결과를 순서대로 기록합니다.
- `demo/`는 각 study에서 다루는 내용을 실제로 구현한 Axum 프로젝트입니다. study가 진행될수록 `demo/` 코드도 함께 발전합니다.
- `project/`는 `studyFinal.md`에 대응하는, `demo/`와는 다른 구조(Cargo 워크스페이스 + SeaORM + 정적 프론트엔드)로 처음부터 다시 설계한 완결된 미니 프로젝트입니다.

## 사전 준비

Rust 툴체인이 필요합니다. (Windows에서는 MSVC 빌드 도구 + Windows SDK도 함께 필요합니다.)

```powershell
# https://rustup.rs 에서 rustup-init.exe 실행, 또는
winget install Rustlang.Rustup
rustc --version
cargo --version
```

## 학습 문서 목차

| 문서 | 주제 | 핵심 내용 |
|---|---|---|
| [study0.md](study0.md) | Axum 기초 이론 | Rust 웹 생태계, tokio/hyper/tower/axum 스택, Router·Handler·Extractor·IntoResponse, Spring·FastAPI·Nest와 비교 |
| [study1.md](study1.md) | 설치 & Hello World | `cargo new`, Cargo.toml, 주요 cargo 명령어, `Router::new().route(...)`, Path/Query extractor |
| [study2.md](study2.md) | 게시판 CRUD (in-memory) | `Arc<RwLock<..>>` 공유 상태, `State` extractor, `Json` 요청/응답, 모듈 분리 |
| [study3.md](study3.md) | JSON & 검증 | serde 세부 제어, `validator` 크레이트, 커스텀 extractor(`ValidatedJson`) |
| [study4.md](study4.md) | 오류 타입으로 응답 통일 | `thiserror` + `AppError` enum + `IntoResponse`, `?` 연산자, fallback / 405 처리 |
| [study5.md](study5.md) | sqlx + SQLite | 마이그레이션, `SqlitePool`, `FromRow`, in-memory → DB 전환 |
| [study6.md](study6.md) | 회원 & JWT 인증 | Argon2 비밀번호 해시, `jsonwebtoken`, `FromRequestParts`로 만든 `AuthUser` extractor |
| [study7.md](study7.md) | tower 미들웨어 계층 | `tower-http`(Trace/Cors/Compression/Timeout), `middleware::from_fn`, 소유권 검사(403) |
| [study8.md](study8.md) | 페이지네이션 & 파일 업로드 | 커서/오프셋 페이지네이션, `multipart` 업로드, `ServeDir` 정적 서빙, `DefaultBodyLimit` |
| [study9.md](study9.md) | 테스트 & 설정 & 로깅 | `tower::ServiceExt::oneshot` 통합 테스트, `.env` 설정 로딩, `tracing` 구조적 로깅 |
| [study10.md](study10.md) | OpenAPI & Docker 배포 | `utoipa` + Swagger UI, 멀티 스테이지 Dockerfile, 릴리스 빌드 최적화 |
| [studyFinal.md](studyFinal.md) | 종합 프로젝트 (구조 다르게) | Cargo 워크스페이스 멀티 크레이트, SeaORM, 정적 프론트엔드까지 붙인 간단한 웹사이트 |

## 저장소 구조

```
Auxm/
├─ study0.md ~ studyFinal.md   ← 학습 문서 (개념 + 실습 + 실제 실행 결과)
├─ demo/                       ← Axum 실습 프로젝트 (study1~study10 누적)
├─ project/                    ← studyFinal 최종 프로젝트 (워크스페이스 + SeaORM, 별도 구조)
└─ .agent/                     ← 여러 AI 에이전트가 이 저장소에서 작업할 때 따르는 공통 가이드라인
```
