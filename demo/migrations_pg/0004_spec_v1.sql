-- migrations/0004_spec_v1.sql(SQLite)과 같은 의미. 차이: IDENTITY, TIMESTAMPTZ
ALTER TABLE posts RENAME COLUMN content TO body;
ALTER TABLE users RENAME COLUMN username TO nickname;

CREATE TABLE IF NOT EXISTS comments (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    post_id    BIGINT NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    author_id  BIGINT NOT NULL REFERENCES users (id),
    body       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_comments_post_id ON comments (post_id);
