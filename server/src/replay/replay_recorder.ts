import { FrameInput } from "../../../shared/protocol/packets";

export interface ReplayHeader {
  matchId: string;
  startedAt: string;
  tickRate: number;
  initialSeed: number;
  players: { id: number; name: string }[];
}

export interface ReplayRecord {
  header: ReplayHeader;
  frames: { frame: number; inputs: { [playerId: number]: FrameInput }; stateChecksum: string }[];
}

export class MatchReplayRecorder {
  private replay: ReplayRecord;

  constructor(matchId: string, initialSeed: number = 42) {
    this.replay = {
      header: {
        matchId,
        startedAt: new Date().toISOString(),
        tickRate: 60,
        initialSeed,
        players: [
          { id: 0, name: "Player 1" },
          { id: 1, name: "Player 2" },
        ],
      },
      frames: [],
    };
  }

  public recordFrame(frame: number, inputs: { [playerId: number]: FrameInput }, stateChecksum: string): void {
    this.replay.frames.push({ frame, inputs, stateChecksum });
  }

  public exportJSON(): string {
    return JSON.stringify(this.replay, null, 2);
  }
}
