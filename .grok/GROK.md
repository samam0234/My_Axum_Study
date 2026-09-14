# Grok 전용 인수인계 문서

공통 규칙은 [../.agent/AGENTS.md](../.agent/AGENTS.md) 및 [../.agent/COMMIT_GUIDE.md](../.agent/COMMIT_GUIDE.md)를 먼저 확인하고 따릅니다.
이 문서에는 Grok을 사용할 때만 해당하는 특이사항을 기록합니다.

## Grok 관련 참고 사항

- 별도의 도구 제약이나 특이사항은 아직 없습니다. 작업 중 Grok 특유의 이슈(툴 제한, 동작 차이 등)를 발견하면 이 섹션에 기록합니다.
- 공통 사항으로, `cargo`가 PATH에 없으면 `%USERPROFILE%\.cargo\bin`을 먼저 PATH에 추가해야 합니다.
