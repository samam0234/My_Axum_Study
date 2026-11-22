CREATE TABLE IF NOT EXISTS users (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL
);

ALTER TABLE posts ADD COLUMN IF NOT EXISTS author_id BIGINT REFERENCES users (id);

-- PostgreSQL은 FK에 인덱스를 자동으로 만들지 않는다. (SQLite도 마찬가지라 같은 문장)
CREATE INDEX IF NOT EXISTS idx_posts_author_id ON posts (author_id);
