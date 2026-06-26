-- Initial database extensions
-- These extensions are required for core functionality

-- UUID generation for primary keys
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Trigram similarity for text search and autocomplete
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

-- Vector similarity (pgvector) - required from day one
CREATE EXTENSION IF NOT EXISTS "vector";
