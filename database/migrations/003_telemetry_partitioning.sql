-- Migration 003: High-Volume Telemetry Range Partitioning
CREATE TABLE IF NOT EXISTS telemetry_events (
    id BIGSERIAL,
    session_id UUID NOT NULL,
    player_id VARCHAR(64) NOT NULL,
    event_type VARCHAR(32) NOT NULL,
    rtt_ms REAL NOT NULL,
    jitter_ms REAL NOT NULL,
    rollback_frames INT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, recorded_at)
) PARTITION BY RANGE (recorded_at);

-- Initial Monthly Partitions
CREATE TABLE IF NOT EXISTS telemetry_events_2026_09 PARTITION OF telemetry_events
    FOR VALUES FROM ('2026-09-01 00:00:00+00') TO ('2026-10-01 00:00:00+00');
CREATE TABLE IF NOT EXISTS telemetry_events_2026_10 PARTITION OF telemetry_events
    FOR VALUES FROM ('2026-10-01 00:00:00+00') TO ('2026-11-01 00:00:00+00');
