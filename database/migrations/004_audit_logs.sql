-- Migration 004: Append-Only Security Audit Logs
CREATE TABLE IF NOT EXISTS security_audit_logs (
    log_id BIGSERIAL PRIMARY KEY,
    actor_id VARCHAR(64) NOT NULL,
    action_type VARCHAR(48) NOT NULL, -- 'LOGIN', 'SESSION_ALLOCATED', 'PLAYER_KICKED', 'PLAYER_BANNED'
    target_id VARCHAR(64),
    ip_address INET,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_actor ON security_audit_logs(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_created ON security_audit_logs(created_at);
