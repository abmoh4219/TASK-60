-- =============================================================================
-- RailOps — V4: contractor user mapping
-- PostgreSQL 16
--
-- Adds user_id to contractors so subscriptions can be keyed to contractor UUID
-- instead of user UUID when subscriber_type = 'contractor'.
-- =============================================================================

ALTER TABLE contractors
    ADD COLUMN IF NOT EXISTS user_id UUID REFERENCES users(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_contractors_user
    ON contractors(user_id)
    WHERE user_id IS NOT NULL;
