-- Migration 001: Initialize core schema with users, matches, sessions

CREATE TABLE users (
    user_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(128) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_email ON users(email);

CREATE TABLE matches (
    match_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_by_user_id UUID NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    status VARCHAR(24) NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'ACTIVE', 'FINISHED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_matches_status ON matches(status);
CREATE INDEX idx_matches_created_by ON matches(created_by_user_id);

CREATE TABLE match_sessions (
    session_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    match_id UUID NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
    room_code VARCHAR(32) NOT NULL UNIQUE,
    status VARCHAR(24) NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'FINISHED')),
    player_count INT NOT NULL DEFAULT 2,
    tick_rate INT NOT NULL DEFAULT 60,
    server_region VARCHAR(16) NOT NULL,
    session_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sessions_match_id ON match_sessions(match_id);
CREATE INDEX idx_sessions_status ON match_sessions(status);
CREATE INDEX idx_sessions_heartbeat ON match_sessions(session_heartbeat) WHERE status = 'ACTIVE';

CREATE TABLE frame_input_logs (
    id BIGSERIAL,
    session_id UUID NOT NULL REFERENCES match_sessions(session_id) ON DELETE CASCADE,
    player_id VARCHAR(64) NOT NULL,
    frame_index BIGINT NOT NULL,
    input_bitmask INT NOT NULL,
    dx_q16 INT NOT NULL,
    dy_q16 INT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, received_at)
);

CREATE INDEX idx_inputs_session_frame ON frame_input_logs(session_id, frame_index);

CREATE TABLE desync_incidents (
    incident_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id UUID NOT NULL REFERENCES match_sessions(session_id) ON DELETE CASCADE,
    frame_index BIGINT NOT NULL,
    server_checksum VARCHAR(64) NOT NULL,
    client_checksum VARCHAR(64) NOT NULL,
    diff_payload JSONB NOT NULL,
    ai_root_cause TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_desync_session ON desync_incidents(session_id);
