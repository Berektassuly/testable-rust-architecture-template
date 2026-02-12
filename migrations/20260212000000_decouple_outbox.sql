-- Decouple Solana outbox scheduling from items table
-- Adds per-outbox scheduling column and optimized polling index

ALTER TABLE solana_outbox
    ADD COLUMN IF NOT EXISTS next_retry_at TIMESTAMPTZ NULL;

-- Optimized index for polling pending outbox entries by schedule
CREATE INDEX IF NOT EXISTS idx_solana_outbox_polling
    ON solana_outbox (status, next_retry_at, created_at)
    WHERE status = 'pending';

