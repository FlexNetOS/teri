-- Users table for identity and authentication

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username TEXT UNIQUE NOT NULL,
    display_name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    settings JSONB DEFAULT '{}'
);

-- Case-insensitive unique index on email
CREATE UNIQUE INDEX idx_users_email ON users(lower(email));

-- Case-insensitive unique index on username
CREATE UNIQUE INDEX idx_users_username ON users(lower(username));

-- Index for faster username lookups
CREATE INDEX idx_users_username_lookup ON users(lower(username));
