-- Create items table with proper indexing
CREATE TABLE IF NOT EXISTS items (
    id VARCHAR(255) PRIMARY KEY,
    hash VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    content TEXT NOT NULL,
    metadata JSONB,
    blockchain_status VARCHAR(50) NOT NULL DEFAULT 'pending',
    blockchain_signature VARCHAR(255),
    blockchain_retry_count INTEGER NOT NULL DEFAULT 0,
    blockchain_last_error TEXT,
    blockchain_next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for frequently accessed fields
CREATE INDEX IF NOT EXISTS idx_items_created_at ON items(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_name ON items(name);
CREATE INDEX IF NOT EXISTS idx_items_hash ON items(hash);
CREATE INDEX IF NOT EXISTS idx_items_blockchain_status ON items(blockchain_status);
CREATE INDEX IF NOT EXISTS idx_items_blockchain_next_retry ON items(blockchain_next_retry_at) 
    WHERE blockchain_status = 'pending_submission';

-- GIN index for JSONB metadata queries
CREATE INDEX IF NOT EXISTS idx_items_metadata ON items USING GIN (metadata);

-- Comment on table
COMMENT ON TABLE items IS 'Core items table with blockchain integration support';
COMMENT ON COLUMN items.blockchain_status IS 'Status: pending, pending_submission, submitted, confirmed, failed';
