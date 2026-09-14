# 에이전트 공통 가이드라인 (인수인계 문서)

이 문서는 이 저장소에서 작업하는 모든 AI 에이전트(Claude, Grok, Codex 등)가 **공통으로** 지켜야 할 규칙을 정리한 인수인계 문서입니다.
`.claude/`, `.grok/`, `.codex/` 폴더의 에이전트별 문서는 이 문서를 기본으로 삼고, 도구별 특이사항만 추가로 기술합니다.

## 1. 저장소 개요

- Rust 기반 웹 프레임워크 [Axum](https://github.com/tokio-rs/axum) 학습 기록 저장소입니다. Axum은 tokio 팀이 만든 프레임워크로, `tower`의 `Service` 추상화 위에 라우터와 extractor를 얹은 구조입니다.
- 같은 워크스페이스의 `Spring_Boot/`, `python/FastApi/`, `nest.js/`와 사실상 동일한 커리큘럼(계층 분리, 요청 파싱, DTO 검증, DB 연동, JWT 인증, 예외 처리, 페이지네이션, 테스트, 문서화, 배포)을 Rust/Axum 방식으로 반복합니다.
- 다른 세 프레임워크와 결정적으로 다른 점은 **DI 컨테이너가 없고, 소유권/생명주기를 타입으로 직접 다뤄야 한다**는 것입니다. `@Autowired`/`@Injectable` 대신 `State<AppState>`를 명시적으로 주입하고, 런타임 예외 대신 `Result<T, AppError>`로 오류를 값으로 다룹니다.
- 공통 스펙: **게시판(Posts) CRUD + 회원/인증**을 처음부터 끝까지 사용합니다. in-memory(`Arc<RwLock<..>>`) → sqlx(SQLite) → JWT 인증 순으로 점진적으로 발전시킵니다.
- 학습 목표와 전체 로드맵은 [Notion: Axum 학습 목표](https://app.notion.com/p/Axum-3d6d7dec5e2e814581a4e96fc36e6c71) 페이지를 기준으로 삼습니다.
- `study0.md`, `study1.md`, `study2.md`, ... 순서로 학습 진도가 진행됩니다. (숫자가 클수록 최신 진도)
  - `study0`: 실습 없이 이론만 (Rust 웹 생태계, tokio/hyper/tower/axum 스택, Router·Handler·Extractor·IntoResponse 개념, 다른 프레임워크와의 비교)
  - `study1` ~ `study5`: Axum 기본기 실습 (설치·cargo 명령어, 라우터/extractor, in-memory 게시판 CRUD, serde+validator 검증, 오류 타입 통일, sqlx DB 연동)
  - `study6` ~ `study10`: 고급 기술 구현 (회원+JWT 인증 extractor, tower 미들웨어 계층·인가, 페이지네이션·파일 업로드, 테스트·설정·tracing, OpenAPI 문서화·Docker 배포)
  - `studyFinal`: 지금까지 배운 것을 응용하되 **구조를 다르게** 설계한 최종 미니 프로젝트 (Cargo 워크스페이스 멀티 크레이트 + sqlx 대신 SeaORM + 정적 프론트엔드 서빙)
- `demo/`: study1~study10 문서 내용을 단계별로 하나씩 실습하며 누적되는 Axum 프로젝트입니다. 커밋 히스토리를 따라가면 배운 순서 그대로 코드가 진화하는 것을 볼 수 있습니다.
- `project/`: `studyFinal.md`에 대응하는, `demo/`에서 익힌 개념을 전부 합쳐 처음부터 다른 구조로 완성한 독립된 미니 프로젝트입니다.

## 2. 작업 전 확인 사항

- 새 작업을 시작하기 전에 `studyN.md` 파일들을 번호 순서대로 확인하여 지금까지의 학습 진도와 맥락을 파악합니다.
- `demo/`, `project/` 프로젝트 코드가 study 문서 설명과 어긋나지 않는지 확인합니다.
- 새 studyN.md 문서를 추가/수정할 때는 이전 study 문서와 이어지는 흐름을 유지합니다.
- Rust 프로젝트는 `target/` 빌드 산출물이 매우 크므로 반드시 `.gitignore`에 포함되어 있어야 합니다. (SQLite DB 파일, `.env`도 마찬가지)
- 코드를 고쳤으면 **커밋 전에 최소한 `cargo check`를 통과**시킵니다. 가능하면 `cargo run`으로 서버를 띄워 `curl`로 실제 응답을 확인한 뒤 커밋합니다.
- Rust는 컴파일 시간이 깁니다. 첫 빌드는 수 분이 걸릴 수 있으니 백그라운드로 돌리고, 의존성을 추가할 때는 한 번에 모아서 추가하는 편이 효율적입니다.
- `cargo run`을 백그라운드로 띄운 채로는 `target/` 안의 실행 파일이 잠겨 `cargo build`가 실패합니다(Windows 파일 잠금). 재빌드 전에 서버 프로세스를 먼저 종료합니다.

## 3. 커밋 규칙

- 커밋 관련 규칙은 [COMMIT_GUIDE.md](./COMMIT_GUIDE.md) 문서를 따릅니다.

## 4. 언어 규칙

- 문서 내용, 커밋 설명: 한글
- 커밋 타입(prefix), 코드, 식별자, 파일명: 영어

## 5. 에이전트별 문서

- Claude: [.claude/CLAUDE.md](../.claude/CLAUDE.md)
- Grok: [.grok/GROK.md](../.grok/GROK.md)
- Codex: [.codex/AGENTS.md](../.codex/AGENTS.md)

## 6. 인수인계 원칙

- 작업을 마칠 때는 다음 에이전트/세션이 이어받을 수 있도록 변경 내용과 다음 단계 후보를 간단히 남깁니다.
- 불확실한 결정은 추측하지 말고 study 문서 내용과 [Notion 학습 목표 페이지](https://app.notion.com/p/Axum-3d6d7dec5e2e814581a4e96fc36e6c71)를 근거로 삼습니다.
- 이 문서나 커밋 가이드에 없는 새로운 컨벤션이 필요해지면, 추측해서 각자 다르게 처리하지 말고 이 문서를 먼저 갱신합니다.
