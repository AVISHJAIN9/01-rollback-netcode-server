export interface MatchSession {
  sessionId: string;
  roomCode: string;
  region: string;
  tickRate: number;
  playerIds: string[];
  reconnectTokens: { [playerId: string]: { token: string; expiresAt: number } };
  spectatorIds: string[];
  status: 'LOBBY' | 'ACTIVE' | 'COMPLETED' | 'PAUSED';
  startedAt: number;
  lastHeartbeat: number;
}

export class SessionManager {
  private sessions: Map<string, MatchSession> = new Map();
  private playerSessions: Map<string, string> = new Map();

  public createSession(roomCode: string, region: string = 'us-east'): MatchSession {
    const sessionId = `sess_${Date.now()}_${Math.random().toString(36).substr(2, 6)}`;
    const session: MatchSession = {
      sessionId,
      roomCode,
      region,
      tickRate: 60,
      playerIds: [],
      reconnectTokens: {},
      spectatorIds: [],
      status: 'LOBBY',
      startedAt: Date.now(),
      lastHeartbeat: Date.now(),
    };
    this.sessions.set(sessionId, session);
    return session;
  }

  public joinSession(sessionId: string, playerId: string, isSpectator: boolean = false): { success: boolean; token?: string; reason?: string } {
    const session = this.sessions.get(sessionId);
    if (!session) return { success: false, reason: "Session not found" };

    if (isSpectator) {
      session.spectatorIds.push(playerId);
      return { success: true };
    }

    if (session.playerIds.length >= 2 && !session.playerIds.includes(playerId)) {
      return { success: false, reason: "Match room full" };
    }

    if (!session.playerIds.includes(playerId)) {
      session.playerIds.push(playerId);
    }

    const token = `rec_${Date.now()}_${Math.random().toString(36).substr(2, 8)}`;
    session.reconnectTokens[playerId] = {
      token,
      expiresAt: Date.now() + 30000, // 30s grace window
    };
    this.playerSessions.set(playerId, sessionId);

    if (session.playerIds.length === 2) {
      session.status = 'ACTIVE';
    }

    return { success: true, token };
  }

  public validateReconnect(sessionId: string, playerId: string, token: string): boolean {
    const session = this.sessions.get(sessionId);
    if (!session) return false;
    const rec = session.reconnectTokens[playerId];
    if (!rec) return false;
    if (Date.now() > rec.expiresAt) return false;
    return rec.token === token;
  }

  public getSession(sessionId: string): MatchSession | undefined {
    return this.sessions.get(sessionId);
  }
}
