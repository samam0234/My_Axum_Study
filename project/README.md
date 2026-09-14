# Rust Board — studyFinal 최종 프로젝트

`demo/`에서 배운 개념을 **구조를 바꿔서** 다시 조립한 미니 게시판 웹사이트입니다.

- **Cargo 워크스페이스** — `domain`(비즈니스 규칙) / `web`(HTTP 표현) 크레이트 분리
- **SeaORM** — `demo/`의 sqlx(쿼리 직접 작성) 대신 ORM
- **HttpOnly 쿠키 세션** — `demo/`의 Bearer 헤더 대신 브라우저 친화적 인증
- **정적 프론트엔드 내장** — 빌드 도구 없이 `cargo run` 하나로 사이트 전체가 뜸

## 실행

```powershell
cargo run -p web
# http://127.0.0.1:4000
```

환경변수는 [.env.example](.env.example) 참고. 아무것도 설정하지 않아도 개발용 기본값으로 동작합니다.

## 구조

```
project/
├─ Cargo.toml              ← 워크스페이스 정의 + 공통 의존성 버전
├─ crates/
│  ├─ domain/              ← HTTP를 전혀 모르는 크레이트
│  │  └─ src/
│  │     ├─ entity/        ← SeaORM 엔티티 (posts, users)
│  │     ├─ migration/     ← Rust 코드로 쓴 마이그레이션
│  │     ├─ service/       ← 비즈니스 규칙 (소유권 검사 포함)
│  │     ├─ error.rs       ← DomainError (StatusCode 없음!)
│  │     └─ password.rs
│  └─ web/                 ← axum 바이너리
│     └─ src/
│        ├─ api/           ← 라우터 + 핸들러
│        ├─ auth.rs        ← JWT + 쿠키 + AuthUser extractor
│        ├─ dto.rs         ← 요청/응답 타입
│        ├─ error.rs       ← DomainError → HTTP 번역 계층
│        └─ main.rs
└─ static/                 ← vanilla JS SPA (index.html / app.js / style.css)
```

## API

| 메서드 | 경로 | 인증 |
|---|---|---|
| GET | `/api/health` | — |
| POST | `/api/auth/register` | — |
| POST | `/api/auth/login` | — (성공 시 세션 쿠키 발급) |
| POST | `/api/auth/logout` | — |
| GET | `/api/auth/me` | 🔒 |
| GET | `/api/posts?page=&per_page=&q=` | — |
| GET | `/api/posts/{id}` | — |
| POST | `/api/posts` | 🔒 |
| PATCH | `/api/posts/{id}` | 🔒 본인 글만 |
| DELETE | `/api/posts/{id}` | 🔒 본인 글만 |

자세한 설명은 [../studyFinal.md](../studyFinal.md)를 참고하세요.
