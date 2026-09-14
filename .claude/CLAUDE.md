# Claude 전용 인수인계 문서

공통 규칙은 [../.agent/AGENTS.md](../.agent/AGENTS.md) 및 [../.agent/COMMIT_GUIDE.md](../.agent/COMMIT_GUIDE.md)를 먼저 확인하고 따릅니다.
이 문서에는 Claude를 사용할 때만 해당하는 특이사항을 기록합니다.

## Claude 관련 참고 사항

- 이 저장소 작업에는 Rust 툴체인이 필요합니다. `rustup`은 `%USERPROFILE%\.cargo\bin`에 설치되며, 새 셸에서 `cargo`를 못 찾으면 PATH에 해당 경로를 먼저 추가하고 실행합니다.
  - PowerShell: `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"`
  - Bash: `export PATH="$USERPROFILE/.cargo/bin:$PATH"`
- Rust는 컴파일이 느립니다. 첫 `cargo build`는 수 분이 걸리므로 `run_in_background`로 돌리고, 그동안 문서 작성 등 다른 작업을 병행하는 편이 효율적입니다.
- `cargo run`으로 서버를 백그라운드에 띄운 상태에서 다시 빌드하면 Windows 파일 잠금 때문에 `target\debug\*.exe` 링크 단계에서 실패합니다(`Access is denied` / `os error 5`). 재빌드 전에 반드시 프로세스를 종료합니다.
- curl로 한글이 포함된 JSON body(`-d '{"title":"한글", ...}'`)를 보낼 때 Windows 터미널 코드페이지 문제로 인코딩이 깨질 수 있습니다. 동작 검증용 smoke test에는 영어 payload를 사용하고, 한글 데이터 검증은 UTF-8로 저장한 파일을 `curl --data-binary @file.json`으로 넘기는 방식이 안전합니다.
- **Windows PowerShell 5.1의 `Get-Content -Raw` / `Set-Content`로 소스 파일을 편집하지 마세요.** `Get-Content`가 BOM 없는 UTF-8 파일을 시스템 ANSI 코드페이지로 읽어버려서 한글 주석이 전부 깨집니다(`議곕┰留?` 같은 문자열이 됨). 파일 수정은 Edit/Write 도구를 쓰거나, 최소한 `-Encoding utf8`을 읽기/쓰기 양쪽에 모두 지정해야 합니다.
- sqlx의 컴파일 타임 검증 매크로(`query!`, `query_as!`)는 빌드 시점에 실제 DB 연결 또는 `.sqlx/` 오프라인 캐시를 요구합니다. 학습용으로는 런타임 검증 함수 버전(`sqlx::query`, `sqlx::query_as::<_, T>`)을 쓰는 편이 환경 의존성이 적어 안전합니다.
- 같은 워크스페이스의 `Spring_Boot/`, `python/FastApi/`, `nest.js/` 학습 저장소와 커리큘럼이 대응되므로, 개념 설명 시 "Spring의 `@RestController` ≈ Axum의 핸들러 함수 + `Router`", "FastAPI의 `Depends` ≈ Axum의 extractor", "Nest의 Guard ≈ Axum의 `FromRequestParts` 또는 tower `Layer`" 식으로 비교해주면 사용자의 기존 지식과 연결하기 좋습니다.
- Axum은 메이저 버전마다 API가 꽤 바뀝니다(0.6 → 0.7에서 `Server` 제거 및 `axum::serve` 도입, 0.8에서 경로 파라미터 문법이 `:id` → `{id}`로 변경). 답변/코드 작성 전에 `Cargo.toml`에 고정된 실제 버전을 먼저 확인합니다.
