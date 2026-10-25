import { RollbackEngine } from "../../shared/simulation/rollback";
import { FrameInput, InputButtons } from "../../shared/protocol/packets";

export class NetcodeClientSDK {
  public engine: RollbackEngine;
  public sessionId: string | null = null;
  public playerId: number = 0;

  constructor(playerId: number = 0) {
    this.playerId = playerId;
    this.engine = new RollbackEngine(playerId);
  }

  public sendInput(buttons: number, analogX: number = 0, analogY: number = 0): FrameInput {
    const input: FrameInput = {
      frame: this.engine.activeState.frameIndex,
      buttons,
      analogX,
      analogY,
    };
    this.engine.advanceFrame(input);
    return input;
  }

  public receiveRemoteInput(frame: number, remotePlayerId: number, input: FrameInput): boolean {
    return this.engine.handleRemoteInput(frame, remotePlayerId, input);
  }

  public getCurrentChecksum(): string {
    return this.engine.activeState.computeChecksum();
  }
}
