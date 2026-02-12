-- Performance indexes to eliminate sequential scans on hot query paths.
-- See: list_items (pagination) and claim_pending_solana_outbox (queue polling).

-- 1. Pagination (list_items): keyset pagination ORDER BY created_at DESC, id DESC
--    Postgres can scan this index in order, avoiding an expensive Sort node.
CREATE INDEX IF NOT EXISTS idx_items_pagination ON items (created_at DESC, id DESC);

-- 2. Queue polling (claim_pending_solana_outbox): pending jobs by schedule, then FIFO
--    Partial index on pending only; next_retry_at included (added in decouple_outbox).
--    ORDER BY next_retry_at ASC NULLS FIRST, created_at ASC matches worker access pattern.
CREATE INDEX IF NOT EXISTS idx_outbox_polling ON solana_outbox (next_retry_at ASC NULLS FIRST, created_at ASC)
    WHERE status = 'pending';
