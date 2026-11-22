# 워크스페이스(domain + web) 멀티 스테이지 빌드. (study10, study16, studyFinal)
#
# demo/의 Dockerfile과 같은 원리인데, 크레이트가 둘이라 "가짜 소스" 파일이 크레이트마다 필요하다.
FROM rust:1.98-slim-bookworm AS builder
WORKDIR /app

# ── 의존성 레이어 캐싱 ───────────────────────────────────────────────────────
# 루트 Cargo.toml/Cargo.lock + 각 크레이트의 Cargo.toml만 먼저 복사하고 빈 소스로 빌드한다.
COPY Cargo.toml Cargo.lock ./
COPY crates/domain/Cargo.toml crates/domain/Cargo.toml
COPY crates/web/Cargo.toml crates/web/Cargo.toml
RUN mkdir -p crates/domain/src crates/web/src \
    && echo '' > crates/domain/src/lib.rs \
    && echo 'fn main() {}' > crates/web/src/main.rs \
    && cargo build --release -p web \
    && rm -rf crates/domain/src crates/web/src

COPY crates ./crates
RUN touch crates/domain/src/lib.rs crates/web/src/main.rs \
    && cargo build --release -p web

# ── 실행 ────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /usr/sbin/nologin appuser
WORKDIR /app

COPY --from=builder /app/target/release/web /usr/local/bin/web
COPY static ./static

RUN mkdir -p /app/data && chown -R appuser:appuser /app
VOLUME ["/app/data"]
USER appuser

# DB 종류는 DATABASE_URL의 스킴으로 정해진다 (sqlite:// | postgres:// | mysql://). 드라이버는 모두 포함되어 있다.
ENV DATABASE_URL="sqlite:///app/data/board.db?mode=rwc" \
    BIND_ADDR="0.0.0.0:4000" \
    STATIC_DIR="/app/static" \
    RUST_LOG="web=info,domain=info,tower_http=info"
# ⚠️ JWT_SECRET은 이미지에 굽지 않는다. SECURE_COOKIE=true는 HTTPS 뒤에서 켠다.

EXPOSE 4000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:4000/api/health || exit 1

CMD ["web"]
