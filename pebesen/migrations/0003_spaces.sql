-- Spaces table for organizing content

CREATE TABLE IF NOT EXISTS spaces (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    slug TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'private', 'secret')),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Case-insensitive unique index on slug
CREATE UNIQUE INDEX idx_spaces_slug ON spaces(lower(slug));

-- Index for faster slug lookups
CREATE INDEX idx_spaces_slug_lookup ON spaces(lower(slug));
