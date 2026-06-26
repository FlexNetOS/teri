-- Streams table for organizing messages within spaces

CREATE TABLE IF NOT EXISTS streams (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    space_id UUID REFERENCES spaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    visibility TEXT DEFAULT 'public' CHECK (visibility IN ('public', 'private')),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (space_id, name)
);

-- Index for faster stream lookups by space
CREATE INDEX idx_streams_space ON streams(space_id);

-- Index for faster stream lookups by space and name
CREATE INDEX idx_streams_space_name ON streams(space_id, name);
