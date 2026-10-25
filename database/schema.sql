-- PostgreSQL Schema for Rollback Netcode Server
CREATE TABLE IF NOT EXISTS match_sessions (
    session_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    room_code VARCHAR(32) NOT NULL UNIQUE,
    player_count INT NOT NULL DEFAULT 2,
    tick_rate INT NOT NULL DEFAULT 60,
    server_region VARCHAR(16) NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    status VARCHAR(24) NOT NULL DEFAULT 'ACTIVE'
);

CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON match_sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON match_sessions(status);

CREATE TABLE IF NOT EXISTS frame_input_logs (
    id BIGSERIAL,
    session_id UUID NOT NULL REFERENCES match_sessions(session_id) ON DELETE CASCADE,
    player_id VARCHAR(64) NOT NULL,
    frame_index BIGINT NOT NULL,
    input_bitmask INT NOT NULL,
    dx_q16 INT NOT NULL,
    dy_q16 INT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, received_at)
) PARTITION BY RANGE (received_at);

CREATE INDEX IF NOT EXISTS idx_inputs_session_frame ON frame_input_logs(session_id, frame_index);

CREATE TABLE IF NOT EXISTS desync_incidents (
    incident_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id UUID NOT NULL REFERENCES match_sessions(session_id) ON DELETE CASCADE,
    frame_index BIGINT NOT NULL,
    server_checksum VARCHAR(64) NOT NULL,
    client_checksum VARCHAR(64) NOT NULL,
    diff_payload JSONB NOT NULL,
    ai_root_cause TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_desync_session ON desync_incidents(session_id);
