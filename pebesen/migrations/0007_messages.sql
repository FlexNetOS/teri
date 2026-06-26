-- Messages table for storing messages within topics

CREATE TABLE IF NOT EXISTS messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    topic_id UUID REFERENCES topics(id) ON DELETE CASCADE NOT NULL,
    author_id UUID REFERENCES users(id) ON DELETE SET NULL NOT NULL,
    content TEXT NOT NULL,
    rendered TEXT,
    edited_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

-- Index for faster message lookups by topic and time
CREATE INDEX idx_messages_topic_time ON messages(topic_id, created_at);

-- Index for faster message lookups by author
CREATE INDEX idx_messages_author ON messages(author_id);

-- Index for filtering non-deleted messages
CREATE INDEX idx_messages_deleted ON messages(deleted_at) WHERE deleted_at IS NULL;

-- Index for edited messages
CREATE INDEX idx_messages_edited ON messages(edited_at) WHERE edited_at IS NOT NULL;
