-- Migration 005: Hot-Path Composite Performance Indexes
CREATE INDEX IF NOT EXISTS idx_sessions_region_status ON match_sessions(server_region, status);
CREATE INDEX IF NOT EXISTS idx_inputs_session_frame_player ON frame_input_logs(session_id, frame_index, player_id);
