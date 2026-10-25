-- Migration 002: Users, Profiles & Auth Credentials
CREATE TABLE IF NOT EXISTS users (
    user_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(32) NOT NULL UNIQUE,
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255),
    role VARCHAR(24) NOT NULL DEFAULT 'PLAYER', -- 'PLAYER', 'MODERATOR', 'ADMIN'
    is_banned BOOLEAN NOT NULL DEFAULT FALSE,
    mmr INT NOT NULL DEFAULT 1200,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_mmr ON users(mmr);
