-- PostgreSQL용 마이그레이션. migrations/(SQLite)와 같은 순서·같은 의미로 유지한다.
-- 차이점: AUTOINCREMENT → IDENTITY, TEXT 날짜 → TIMESTAMPTZ
CREATE TABLE IF NOT EXISTS posts (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    author     TEXT NOT NULL,
    -- SQLite와 같은 코드(PostRow.tags: String)를 쓰기 위해 JSON 문자열로 둔다.
    -- PostgreSQL이라면 TEXT[] 또는 JSONB가 더 자연스럽다. (study11 직접 해보기)
    tags       TEXT NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
