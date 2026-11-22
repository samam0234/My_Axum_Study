# Rust Board — 최종 프로젝트 (studyFinal)

`demo/`에서 배운 개념을 **구조를 바꿔서** 다시 조립한 미니 게시판 웹사이트입니다. 설명은 [../studyFinal.md](../studyFinal.md)입니다.

- **Cargo 워크스페이스** — `domain`(비즈니스 규칙) / `web`(HTTP 표현) 크레이트 분리
- **SeaORM** — `demo/`의 sqlx(쿼리 직접 작성) 대신 ORM. **DB 종류는 `DATABASE_URL`의 스킴으로** 정해진다 (SQLite · PostgreSQL · MariaDB 드라이버 포함, 실제 연결 검증)
- **HttpOnly 쿠키 세션** — `demo/`의 Bearer 헤더 대신 브라우저 친화적 인증 (Bearer 헤더도 받는다)
- **정적 프론트엔드 내장** — 빌드 도구 없이 `cargo run` 하나로 사이트 전체가 뜸
- **Docker** — 웹 + PostgreSQL을 `docker compose`로

## 실행

```powershell
cargo run -p web
# http://127.0.0.1:4000
```

환경변수는 [.env.example](.env.example) 참고. 아무것도 설정하지 않아도 개발용 기본값으로 동작합니다(개발용 `JWT_SECRET`이면 경고가 출력됩니다).

DB 바꾸기 (코드 변경 없음):

```powershell
$env:DATABASE_URL = "postgres://postgres:postgres@127.0.0.1:55432/board"   # PostgreSQL
$env:DATABASE_URL = "mysql://root:pw@127.0.0.1:53306/board"                # MariaDB
cargo run -p web
```

### Docker (웹 + PostgreSQL)

```powershell
$env:JWT_SECRET = python -c "import secrets;print(secrets.token_urlsafe(48))"
docker compose up -d --build        # http://127.0.0.1:4000  (WEB_PORT로 바꿀 수 있다)
docker compose down                 # 데이터 볼륨은 남는다 (-v를 붙이면 지운다)
```

`JWT_SECRET`이 없으면 컨테이너 시작을 거부합니다. HTTPS 프록시 뒤에서는 `SECURE_COOKIE=true`.

## 테스트

```powershell
cargo test --workspace      # 도메인 서비스 시험 5 (소유권·중복 가입·검색 대소문자·페이지네이션·로그인) + 유틸 1
```

도메인 크레이트가 HTTP를 모르기 때문에 서버를 띄우지 않고 서비스 함수를 직접 시험합니다.

## 구조

```
project/
├─ Cargo.toml              ← 워크스페이스 정의 + 공통 의존성 버전
├─ Dockerfile  compose.yaml  .dockerignore
├─ crates/
│  ├─ domain/              ← HTTP를 전혀 모르는 크레이트
│  │  ├─ src/
│  │  │  ├─ entity/        ← SeaORM 엔티티 (posts, users)
│  │  │  ├─ migration/     ← Rust 코드로 쓴 마이그레이션 (DB 공통)
│  │  │  ├─ service/       ← 비즈니스 규칙 (소유권 검사 포함)
│  │  │  ├─ error.rs       ← DomainError (StatusCode 없음!)
│  │  │  └─ password.rs
│  │  └─ tests/services.rs ← 서비스 시험
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

> 이 API의 응답·오류 모양은 **공통 API 스펙 v1과 아직 다릅니다**(목록 `items`/`per_page` snake_case, 오류 `{ error: "code", message }`). 스펙에 맞춘 쪽은 `demo/`의 오류 본문이고, 차이 표와 이 프로젝트를 스펙에 맞추는 순서는 [../study6.md](../study6.md) ④와 [../studyFinal.md](../studyFinal.md)에 있습니다.
