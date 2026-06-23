-- Topics table for organizing discussions within streams

CREATE TABLE IF NOT EXISTS topics (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    stream_id UUID REFERENCES streams(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT DEFAULT 'open' CHECK (status IN ('open', 'closed', 'archived')),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    last_active TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (stream_id, name)
);

-- Index for faster topic lookups by stream
CREATE INDEX idx_topics_stream ON topics(stream_id);

-- Index for faster topic lookups by stream and name
CREATE INDEX idx_topics_stream_name ON topics(stream_id, name);

-- Index for faster topic lookups by status
CREATE INDEX idx_topics_status ON topics(status);

-- Index for faster topic lookups by created_by
CREATE INDEX idx_topics_created_by ON topics(created_by);

-- Index for faster topic lookups by last_active
CREATE INDEX idx_topics_last_active ON topics(last_active);
