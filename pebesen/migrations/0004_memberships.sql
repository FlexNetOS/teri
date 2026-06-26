-- Memberships table linking users to spaces with roles

CREATE TABLE IF NOT EXISTS memberships (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    space_id UUID REFERENCES spaces(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'editor', 'viewer')),
    joined_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (user_id, space_id)
);

-- Index for querying members of a space
CREATE INDEX idx_memberships_space ON memberships(space_id);

-- Index for querying spaces a user belongs to
CREATE INDEX idx_memberships_user ON memberships(user_id);
