-- Add updated_at to solana_outbox for zombie task recovery.
-- Processing items older than 5 minutes can be reclaimed (worker crashed).

ALTER TABLE solana_outbox
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

COMMENT ON COLUMN solana_outbox.updated_at IS 'Last status update; used to reclaim processing items after worker crash';
