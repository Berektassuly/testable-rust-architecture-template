-- Sticky blockhash for idempotent transaction retries
-- Pins the blockhash used for the first attempt so retries reuse it and avoid double-spending

ALTER TABLE solana_outbox
    ADD COLUMN IF NOT EXISTS attempt_blockhash VARCHAR(64) NULL;

COMMENT ON COLUMN solana_outbox.attempt_blockhash IS 'Blockhash used for this attempt; reused on retries until expired';
