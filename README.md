# Axum 학습 저장소

Rust 웹 프레임워크 [Axum](https://github.com/tokio-rs/axum)을 기초 개념부터 단계별로 실습하며 정리하는 개인 학습 저장소입니다.
같은 워크스페이스의 `Spring_Boot/`, `python/FastApi/`, `nest.js/`와 **동일한 커리큘럼(게시판 API + 인증)** 을 Rust 방식으로 다시 구현하면서, DI 컨테이너도 런타임 예외도 없는 언어에서 웹 서버를 짜면 무엇이 달라지는지를 비교하는 것이 목표입니다.

- `studyN.md` 문서에 개념 정리와 실습 과정, **실제로 실행해서 확인한 결과**를 순서대로 기록합니다.
- `demo/`는 각 study에서 다루는 내용을 구현한 Axum 프로젝트입니다. study가 진행될수록 코드도 함께 발전합니다.
- `project/`는 `studyFinal.md`에 대응하는, `demo/`와는 **다른 구조**(Cargo 워크스페이스 + SeaORM + 정적 프론트엔드)로 처음부터 다시 설계한 완결된 미니 웹사이트입니다.

## 사전 준비

Rust 툴체인이 필요합니다. Windows에서는 **MSVC 빌드 도구 + Windows SDK**(Visual Studio의 "C++를 사용한 데스크톱 개발" 워크로드)도 함께 필요합니다.

```powershell
winget install Rustlang.Rustup   # 또는 https://rustup.rs
rustc --version                  # rustc 1.98.1
cargo --version                  # cargo 1.98.1
```

## 기술 스택

### demo/ (study1 ~ study10)

- Rust 2024 edition, axum 0.8, tokio 1.53
- **sqlx 0.9** + SQLite (SQL 직접 작성, WAL 모드)
- serde + **validator** (DTO 검증), **thiserror / anyhow** (오류 타입)
- **jsonwebtoken** (JWT), **argon2** (비밀번호 해시)
- **tower / tower-http** (요청 ID, 로깅, 패닉 처리, 타임아웃, 압축, CORS, 정적 서빙)
- **tracing** + tracing-subscriber (구조적 로깅)
- **utoipa** + Swagger UI (OpenAPI 문서화)
- 테스트: `tower::ServiceExt::oneshot` 기반 통합 테스트 (18개)

### project/ (studyFinal)

- **Cargo 워크스페이스** — `domain`(비즈니스 규칙) / `web`(HTTP) 크레이트 분리
- **SeaORM 1.1** + SQLite (엔티티·관계·마이그레이션을 Rust 코드로)
- **HttpOnly 쿠키 세션** 인증
- 빌드 도구 없는 **vanilla JS SPA** 내장 (`static/`)

## 프로젝트 실행

```powershell
cd demo
cargo run
```

기본 포트는 `3000`입니다.

- REST API: `http://127.0.0.1:3000`
- API 문서(Swagger UI): `http://127.0.0.1:3000/docs` (study10 이후)
- OpenAPI JSON: `http://127.0.0.1:3000/api-docs/openapi.json`
- DB 파일: `demo/data.db` (서버를 껐다 켜도 데이터 유지 — 초기화하려면 `data.db*`를 지우고 재실행)

최종 프로젝트는 별도로 실행합니다.

```powershell
cd project
cargo run -p web
# http://127.0.0.1:4000  ← API와 웹사이트가 같이 뜹니다
```

## 테스트 실행

```powershell
cd demo
cargo test                 # 유닛 5 + 통합 13
cargo test -- --nocapture  # 로그까지 보기
```

## 학습 문서 목차

| 문서 | 주제 | 핵심 내용 |
|---|---|---|
| [study0.md](study0.md) | Axum 기초 이론 | Rust 웹 생태계, tokio/hyper/tower/axum 스택, Router·Handler·Extractor·IntoResponse, Spring·FastAPI·Nest와 비교, 용어 사전 |
| [study1.md](study1.md) | 설치 & Hello World | `cargo new`, Cargo.toml과 feature, 주요 cargo 명령어, `Path`/`Query` extractor, `{name}` 경로 문법 |
| [study2.md](study2.md) | 게시판 CRUD (in-memory) | `Arc<RwLock<T>>` 공유 상태, `State` extractor, 모듈 분리, `Router<AppState>` vs `Router<()>` |
| [study3.md](study3.md) | JSON & 검증 | serde 어트리뷰트, `validator` 규칙, 커스텀 extractor `ValidatedJson` |
| [study4.md](study4.md) | 오류 응답 통일 | `thiserror` + `AppError` + `IntoResponse`, extractor rejection 흡수, 404/405 fallback |
| [study5.md](study5.md) | sqlx + SQLite | 마이그레이션, 커넥션 풀(WAL/busy_timeout), `RETURNING`·`COALESCE`, `PostRow` ↔ `Post` |
| [study6.md](study6.md) | 회원 & JWT 인증 | Argon2 + `spawn_blocking`, JWT, `FromRequestParts`로 만든 `AuthUser`, 인증(401)/인가(403) |
| [study7.md](study7.md) | tower 미들웨어 계층 | `ServiceBuilder` 순서, 요청 ID, `TraceLayer`, `CatchPanicLayer`, 타임아웃, 압축, CORS |
| [study8.md](study8.md) | 페이지네이션 & 업로드 | `QueryBuilder`로 안전한 동적 SQL, multipart 업로드, 경로 조작 방어, `ServeDir` |
| [study9.md](study9.md) | 테스트 & 설정 & 로깅 | lib+bin 구조 전환, `Config`, `oneshot` 통합 테스트, in-memory DB, graceful shutdown |
| [study10.md](study10.md) | OpenAPI & Docker | `utoipa` + Swagger UI, 릴리스 빌드 최적화, 멀티 스테이지 Dockerfile, 의존성 레이어 캐싱 |
| [studyFinal.md](studyFinal.md) | 종합 프로젝트 (구조 다르게) | 워크스페이스 크레이트 분리, SeaORM, 도메인/HTTP 오류 계층 분리, 쿠키 세션, SPA 서빙 |

## 저장소 구조

```
Auxm/
├─ study0.md ~ studyFinal.md   ← 학습 문서 (개념 + 실습 + 실제 실행 결과)
├─ demo/                       ← Axum 실습 프로젝트 (study1~study10 누적)
│  ├─ migrations/              ← sqlx 마이그레이션 SQL
│  ├─ src/                     ← lib.rs(앱 조립) + main.rs(실행) + 기능 모듈
│  ├─ tests/api.rs             ← oneshot 통합 테스트
│  └─ Dockerfile               ← 멀티 스테이지 배포
├─ project/                    ← studyFinal 최종 프로젝트 (워크스페이스 + SeaORM + SPA)
│  ├─ crates/domain/           ← HTTP를 모르는 도메인 크레이트
│  ├─ crates/web/              ← axum 표현 계층
│  └─ static/                  ← vanilla JS 프론트엔드
└─ .agent/                     ← 여러 AI 에이전트가 이 저장소에서 작업할 때 따르는 공통 가이드라인
```
