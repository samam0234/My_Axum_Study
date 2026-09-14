# ---------------------------------------------------------------------------
# 1단계: 빌드
#
# Rust 빌드에는 컴파일러·의존성 소스·중간 산출물이 필요하다. 전부 합쳐 수 GB가 된다.
# 하지만 실행에는 **바이너리 하나**면 충분하다. 그래서 멀티 스테이지로 나눈다.
# ---------------------------------------------------------------------------
FROM rust:1.98-slim-bookworm AS builder

WORKDIR /app

# ── 의존성 레이어 캐싱 트릭 ────────────────────────────────────────────────
# Cargo.toml/Cargo.lock만 먼저 복사하고 빈 소스로 한 번 빌드하면,
# 의존성 컴파일 결과가 Docker 레이어로 캐시된다.
# 이후 내 소스만 바뀔 때는 의존성을 다시 컴파일하지 않는다.
# (Rust는 컴파일이 느려서 이 트릭의 효과가 특히 크다)
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release \
    && rm -rf src

# 실제 소스 복사 후 재빌드
COPY src ./src
COPY migrations ./migrations

# 더미 빌드의 산출물이 남아 있으면 새 소스를 반영하지 않으므로 타임스탬프를 갱신한다.
RUN touch src/main.rs src/lib.rs \
    && cargo build --release

# ---------------------------------------------------------------------------
# 2단계: 실행
#
# 빌드 도구가 전혀 없는 작은 베이스 이미지에 바이너리만 옮긴다.
# `sqlx::migrate!`가 마이그레이션 SQL을 바이너리에 박아 넣었으므로
# migrations/ 폴더를 복사할 필요가 없다. (study5 참고)
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# ca-certificates: 외부 HTTPS 호출용
# curl: HEALTHCHECK에서 /health를 찔러보기 위해 (없으면 헬스체크를 못 한다)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# root로 실행하지 않는다. 컨테이너가 뚫려도 피해를 줄인다.
RUN useradd --create-home --shell /usr/sbin/nologin appuser
WORKDIR /app

COPY --from=builder /app/target/release/demo /usr/local/bin/demo

# 데이터와 업로드 파일이 저장될 위치. 볼륨으로 마운트해야 컨테이너를 지워도 살아남는다.
RUN mkdir -p /app/data /app/uploads && chown -R appuser:appuser /app
VOLUME ["/app/data", "/app/uploads"]

USER appuser

ENV DATABASE_URL="sqlite:///app/data/data.db?mode=rwc" \
    UPLOAD_DIR="/app/uploads" \
    BIND_ADDR="0.0.0.0:3000" \
    RUST_LOG="demo=info,tower_http=info"

# ⚠️ JWT_SECRET은 이미지에 굽지 않는다. 실행할 때 -e 로 주입한다.

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1

CMD ["demo"]
