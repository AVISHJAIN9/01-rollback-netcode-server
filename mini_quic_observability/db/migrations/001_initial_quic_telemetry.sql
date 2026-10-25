-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Mini-QUIC Observability: PostgreSQL Schema
-- RFC 9000 Connection, Stream, Loss, and RTT telemetry storage
-- Supports partitioning and long-term analytics

-- Create extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "btree_gist";

-- Connections table: Partitioned by day for retention policies
CREATE TABLE IF NOT EXISTS connections (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    local_addr INET NOT NULL,
    remote_addr INET NOT NULL,
    connection_id BYTEA NOT NULL,
    version BIGINT NOT NULL,
    
    -- Connection state
    state VARCHAR(32) NOT NULL DEFAULT 'handshake',
    handshake_duration_us BIGINT,
    establishment_latency_us BIGINT,
    
    -- Telemetry snapshots
    total_packets_sent BIGINT DEFAULT 0,
    total_packets_received BIGINT DEFAULT 0,
    total_bytes_sent BIGINT DEFAULT 0,
    total_bytes_received BIGINT DEFAULT 0,
    total_packets_lost BIGINT DEFAULT 0,
    
    -- Congestion & RTT
    final_cwnd_bytes BIGINT,
    final_srtt_us BIGINT,
    final_min_rtt_us BIGINT,
    peak_bandwidth_mbps NUMERIC(10, 2),
    
    -- Lifecycle
    closed_at TIMESTAMP WITH TIME ZONE,
    close_reason VARCHAR(256),
    
    -- Metadata
    user_agent VARCHAR(512),
    experiment_tag VARCHAR(256),
    
    INDEX idx_connections_created_at (created_at),
    INDEX idx_connections_remote_addr (remote_addr),
    INDEX idx_connections_state (state)
) PARTITION BY RANGE (created_at);

-- Create daily partitions for the last 90 days
DO $$
DECLARE
    partition_date DATE := CURRENT_DATE - INTERVAL '90 days';
    partition_name VARCHAR;
    start_date DATE;
    end_date DATE;
BEGIN
    FOR i IN 0..90 LOOP
        start_date := partition_date + (i || ' days')::INTERVAL;
        end_date := partition_date + ((i+1) || ' days')::INTERVAL;
        partition_name := 'connections_' || TO_CHAR(start_date, 'YYYY_MM_DD');
        
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF connections
             FOR VALUES FROM (%L) TO (%L)',
            partition_name,
            start_date,
            end_date
        );
    END LOOP;
END $$;

-- Streams table
CREATE TABLE IF NOT EXISTS streams (
    id BIGSERIAL PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Stream metadata (RFC 9000 §2.1)
    stream_id BIGINT NOT NULL,
    stream_type VARCHAR(32) NOT NULL, -- 'bidirectional' or 'unidirectional'
    initiator VARCHAR(16) NOT NULL,  -- 'client' or 'server'
    
    -- State & lifecycle
    state VARCHAR(32) NOT NULL DEFAULT 'open',
    local_finished BOOLEAN DEFAULT FALSE,
    remote_finished BOOLEAN DEFAULT FALSE,
    closed_at TIMESTAMP WITH TIME ZONE,
    
    -- Flow control & buffering
    max_stream_data_sent BIGINT,
    max_stream_data_received BIGINT,
    final_offset BIGINT,
    
    -- Delivery metrics
    bytes_sent BIGINT DEFAULT 0,
    bytes_received BIGINT DEFAULT 0,
    receive_gaps_count INT DEFAULT 0,      -- Out-of-order gaps
    head_of_line_delay_us BIGINT,           -- HOL blocking delay
    
    -- Retransmissions
    retransmit_count BIGINT DEFAULT 0,
    
    UNIQUE (connection_id, stream_id),
    INDEX idx_streams_connection_id (connection_id),
    INDEX idx_streams_state (state),
    INDEX idx_streams_created_at (created_at)
);

-- Loss events table: Partitioned by day
CREATE TABLE IF NOT EXISTS loss_events (
    id BIGSERIAL PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    occurred_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Loss detection method (RFC 9002 §6)
    detection_method VARCHAR(32) NOT NULL, -- 'packet_threshold' or 'time_threshold'
    packet_threshold_count INT,             -- For packet-threshold loss
    time_threshold_multiplier NUMERIC(5, 2), -- For time-threshold (e.g., 1.125)
    
    -- Lost packets
    lost_packet_numbers BIGINT[] DEFAULT ARRAY[]::BIGINT[],
    lost_packet_count INT NOT NULL,
    total_bytes_lost BIGINT NOT NULL,
    
    -- Recovery state
    ssthresh_bytes BIGINT,
    cwnd_before_recovery BIGINT,
    cwnd_after_recovery BIGINT,
    
    -- Telemetry at time of loss
    rtt_us BIGINT,
    srtt_us BIGINT,
    inflight_bytes BIGINT,
    
    INDEX idx_loss_events_connection_id (connection_id),
    INDEX idx_loss_events_occurred_at (occurred_at)
) PARTITION BY RANGE (occurred_at);

-- Create daily partitions for loss_events
DO $$
DECLARE
    partition_date DATE := CURRENT_DATE - INTERVAL '90 days';
    partition_name VARCHAR;
    start_date DATE;
    end_date DATE;
BEGIN
    FOR i IN 0..90 LOOP
        start_date := partition_date + (i || ' days')::INTERVAL;
        end_date := partition_date + ((i+1) || ' days')::INTERVAL;
        partition_name := 'loss_events_' || TO_CHAR(start_date, 'YYYY_MM_DD');
        
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF loss_events
             FOR VALUES FROM (%L) TO (%L)',
            partition_name,
            start_date,
            end_date
        );
    END LOOP;
END $$;

-- RTT samples table: Time-series for RTT tracking
CREATE TABLE IF NOT EXISTS rtt_samples (
    id BIGSERIAL PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    sampled_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- RTT components (RFC 9002 §5.3)
    sample_rtt_us BIGINT NOT NULL,
    srtt_us BIGINT NOT NULL,
    rttvar_us BIGINT NOT NULL,
    min_rtt_us BIGINT,
    max_rtt_us BIGINT,
    
    -- ACK delay
    ack_delay_us BIGINT,
    max_ack_delay_us BIGINT,
    
    INDEX idx_rtt_samples_connection_id (connection_id),
    INDEX idx_rtt_samples_sampled_at (sampled_at)
);

-- Bandwidth measurements table
CREATE TABLE IF NOT EXISTS bandwidth_measurements (
    id BIGSERIAL PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    measured_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Bandwidth estimate (bytes/sec)
    bandwidth_bps BIGINT NOT NULL,
    
    -- Congestion window state at measurement time
    cwnd_bytes BIGINT NOT NULL,
    bytes_in_flight BIGINT NOT NULL,
    
    -- Delivery rate
    bytes_delivered BIGINT,
    delivery_interval_us BIGINT,
    
    -- Pacing rate
    pacing_rate_bps BIGINT,
    
    INDEX idx_bandwidth_measurements_connection_id (connection_id),
    INDEX idx_bandwidth_measurements_measured_at (measured_at)
);

-- Congestion events table: Tracks state machine transitions
CREATE TABLE IF NOT EXISTS congestion_events (
    id BIGSERIAL PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    occurred_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- State transition
    previous_state VARCHAR(32),
    new_state VARCHAR(32) NOT NULL,
    trigger VARCHAR(256) NOT NULL,
    
    -- Metrics at transition time
    cwnd_bytes BIGINT,
    ssthresh_bytes BIGINT,
    rtt_us BIGINT,
    
    INDEX idx_congestion_events_connection_id (connection_id),
    INDEX idx_congestion_events_occurred_at (occurred_at)
);

-- QLOG event storage (optional for raw event streaming)
CREATE TABLE IF NOT EXISTS qlog_events (
    id BIGSERIAL PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    occurred_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Event type (RFC draft-ietf-quic-qlog)
    event_type VARCHAR(64) NOT NULL,
    event_data JSONB NOT NULL,
    
    INDEX idx_qlog_events_connection_id (connection_id),
    INDEX idx_qlog_events_occurred_at (occurred_at),
    INDEX idx_qlog_events_type (event_type)
);

-- Experiment results table
CREATE TABLE IF NOT EXISTS experiment_results (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Experiment metadata
    experiment_name VARCHAR(256) NOT NULL,
    run_number INT NOT NULL,
    algorithm VARCHAR(64) NOT NULL, -- 'newreno', 'cubic', 'rl', etc.
    
    -- Conditions
    latency_ms NUMERIC(10, 2),
    jitter_ms NUMERIC(10, 2),
    packet_loss_percent NUMERIC(5, 2),
    bandwidth_mbps NUMERIC(10, 2),
    
    -- Results
    total_connections INT,
    avg_throughput_mbps NUMERIC(10, 2),
    avg_latency_ms NUMERIC(10, 2),
    packet_loss_count BIGINT,
    congestion_events_count INT,
    
    -- Comparisons
    vs_newreno_improvement_percent NUMERIC(5, 2),
    vs_cubic_improvement_percent NUMERIC(5, 2),
    
    notes TEXT,
    
    UNIQUE (experiment_name, run_number, algorithm),
    INDEX idx_experiment_results_created_at (created_at),
    INDEX idx_experiment_results_experiment_name (experiment_name)
);

-- View: Connection summary statistics
CREATE OR REPLACE VIEW v_connection_summary AS
SELECT
    c.id,
    c.created_at,
    c.local_addr,
    c.remote_addr,
    c.state,
    c.handshake_duration_us / 1000.0 AS handshake_duration_ms,
    c.establishment_latency_us / 1000.0 AS establishment_latency_ms,
    c.total_packets_sent,
    c.total_packets_received,
    c.total_bytes_sent,
    c.total_bytes_received,
    c.total_packets_lost,
    CASE 
        WHEN c.total_packets_sent > 0 
        THEN (c.total_packets_lost::NUMERIC / c.total_packets_sent * 100)
        ELSE 0
    END AS loss_rate_percent,
    c.final_cwnd_bytes,
    c.final_srtt_us / 1000.0 AS final_srtt_ms,
    c.final_min_rtt_us / 1000.0 AS final_min_rtt_ms,
    c.peak_bandwidth_mbps,
    c.closed_at,
    EXTRACT(EPOCH FROM (COALESCE(c.closed_at, NOW()) - c.created_at)) AS duration_seconds,
    COUNT(DISTINCT s.id) AS total_streams
FROM connections c
LEFT JOIN streams s ON c.id = s.connection_id
GROUP BY c.id;

-- View: Stream delivery statistics
CREATE OR REPLACE VIEW v_stream_delivery_stats AS
SELECT
    s.id,
    s.connection_id,
    s.stream_id,
    s.stream_type,
    s.state,
    s.bytes_sent,
    s.bytes_received,
    s.receive_gaps_count,
    s.head_of_line_delay_us / 1000.0 AS head_of_line_delay_ms,
    s.retransmit_count,
    CASE
        WHEN s.bytes_sent > 0
        THEN ((s.bytes_sent - COALESCE(s.bytes_received, 0))::NUMERIC / s.bytes_sent * 100)
        ELSE 0
    END AS loss_percent
FROM streams s;

-- Indexes for common queries
CREATE INDEX idx_connections_state_created ON connections(state, created_at DESC);
CREATE INDEX idx_streams_connection_state ON streams(connection_id, state);
CREATE INDEX idx_loss_events_connection_time ON loss_events(connection_id, occurred_at DESC);
CREATE INDEX idx_rtt_samples_connection_time ON rtt_samples(connection_id, sampled_at DESC);

-- Grant permissions
GRANT SELECT, INSERT, UPDATE ON connections TO PUBLIC;
GRANT SELECT, INSERT, UPDATE ON streams TO PUBLIC;
GRANT SELECT, INSERT ON loss_events TO PUBLIC;
GRANT SELECT, INSERT ON rtt_samples TO PUBLIC;
GRANT SELECT, INSERT ON bandwidth_measurements TO PUBLIC;
GRANT SELECT, INSERT ON congestion_events TO PUBLIC;
GRANT SELECT, INSERT ON qlog_events TO PUBLIC;
GRANT SELECT, INSERT ON experiment_results TO PUBLIC;
GRANT SELECT ON v_connection_summary TO PUBLIC;
GRANT SELECT ON v_stream_delivery_stats TO PUBLIC;
