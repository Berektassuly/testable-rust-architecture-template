-- Create Solana outbox status enum
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'solana_outbox_status') THEN
        CREATE TYPE solana_outbox_status AS ENUM ('pending', 'processing', 'completed', 'failed');
    END IF;
END$$;

-- Create outbox table for Solana submissions
CREATE TABLE IF NOT EXISTS solana_outbox (
    id UUID PRIMARY KEY,
    aggregate_id VARCHAR(255) NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    status solana_outbox_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    retry_count INTEGER NOT NULL DEFAULT 0
);

-- Indexes for efficient polling
CREATE INDEX IF NOT EXISTS idx_solana_outbox_status_created_at
    ON solana_outbox(status, created_at);
CREATE INDEX IF NOT EXISTS idx_solana_outbox_aggregate_id
    ON solana_outbox(aggregate_id);

-- Comments
COMMENT ON TABLE solana_outbox IS 'Outbox table for Solana transaction intents';
COMMENT ON COLUMN solana_outbox.status IS 'Status: pending, processing, completed, failed';
