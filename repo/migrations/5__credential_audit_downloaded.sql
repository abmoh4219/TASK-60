-- Migration 5: Add 'downloaded' to credential_audit_log action CHECK constraint.
--
-- The download_credential handler inserts action='downloaded' which was missing
-- from the original inline CHECK.  PostgreSQL auto-names inline column CHECKs as
-- <table>_<column>_check, so we drop and recreate with the extended value list.

ALTER TABLE credential_audit_log
    DROP CONSTRAINT IF EXISTS credential_audit_log_action_check;

ALTER TABLE credential_audit_log
    ADD CONSTRAINT credential_audit_log_action_check
    CHECK (action IN (
        'uploaded', 'viewed', 'approved', 'rejected',
        'watermarked', 'esigned', 'downloaded'
    ));
