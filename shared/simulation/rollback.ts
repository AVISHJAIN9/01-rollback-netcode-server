import { WorldState } from "./state";
import { SnapshotRingBuffer } from "./ring_buffer";
import { FrameInput } from "../protocol/packets";

export interface RollbackMetrics {
  totalRollbacks: number;
  lastRollbackFrames: number;
  maxRollbackFrames: number;
  desyncDetected: boolean;
  divergingFrame: number | null;
}

export class RollbackEngine {
  public ringBuffer: SnapshotRingBuffer;
  public inputHistory: Map<number, { [playerId: number]: FrameInput }>;
  public activeState: WorldState;
  public localPlayerId: number;
  public metrics: RollbackMetrics;

  constructor(localPlayerId: number = 0) {
    this.localPlayerId = localPlayerId;
    this.ringBuffer = new SnapshotRingBuffer();
    this.inputHistory = new Map();
    this.activeState = new WorldState(0);
    this.metrics = {
      totalRollbacks: 0,
      lastRollbackFrames: 0,
      maxRollbackFrames: 0,
      desyncDetected: false,
      divergingFrame: null,
    };

    // Save initial frame 0
    this.ringBuffer.save(0, this.activeState, this.activeState.computeChecksum());
  }

  public registerInput(frame: number, playerId: number, input: FrameInput): void {
    if (!this.inputHistory.has(frame)) {
      this.inputHistory.set(frame, {});
    }
    this.inputHistory.get(frame)![playerId] = input;
  }

  /**
   * Advances simulation one frame forward with local prediction.
   */
  public advanceFrame(localInput: FrameInput): WorldState {
    const currentFrame = this.activeState.frameIndex;
    this.registerInput(currentFrame, this.localPlayerId, localInput);

    const frameInputs = this.getPredictedInputsForFrame(currentFrame);
    this.activeState.stepPhysics(frameInputs);

    const checksum = this.activeState.computeChecksum();
    this.ringBuffer.save(this.activeState.frameIndex, this.activeState, checksum);

    return this.activeState;
  }

  /**
   * Handles late remote input packet.
   * If remote input differs from predicted input, rewinds to remoteFrame and resimulates forward.
   */
  public handleRemoteInput(remoteFrame: number, remotePlayerId: number, remoteInput: FrameInput): boolean {
    const currentFrame = this.activeState.frameIndex;
    if (remoteFrame >= currentFrame) {
      // Future or current input, simply record
      this.registerInput(remoteFrame, remotePlayerId, remoteInput);
      return false;
    }

    if (remoteFrame + SnapshotRingBuffer.CAPACITY < currentFrame) {
      console.warn("Input too old to rollback (exceeds 128 frames window)");
      return false;
    }

    // Check if recorded prediction matches actual remote input
    const existingInputs = this.inputHistory.get(remoteFrame);
    const prevInput = existingInputs ? existingInputs[remotePlayerId] : null;
    if (prevInput && prevInput.buttons === remoteInput.buttons) {
      // Prediction was accurate! No rollback required.
      return false;
    }

    // Rollback required!
    this.registerInput(remoteFrame, remotePlayerId, remoteInput);
    this.resimulate(remoteFrame, currentFrame);
    return true;
  }

  private resimulate(fromFrame: number, toFrame: number): void {
    const snapshot = this.ringBuffer.get(fromFrame);
    if (!snapshot) {
      console.error(`Cannot rollback: Snapshot for frame ${fromFrame} not found in ring buffer.`);
      return;
    }

    const rollbackDepth = toFrame - fromFrame;
    this.metrics.totalRollbacks++;
    this.metrics.lastRollbackFrames = rollbackDepth;
    this.metrics.maxRollbackFrames = Math.max(this.metrics.maxRollbackFrames, rollbackDepth);

    // 1. Rewind
    this.activeState = snapshot.clone();

    // 2. Resimulate forward
    for (let f = fromFrame; f < toFrame; f++) {
      const inputs = this.getPredictedInputsForFrame(f);
      this.activeState.stepPhysics(inputs);
      const checksum = this.activeState.computeChecksum();
      this.ringBuffer.save(this.activeState.frameIndex, this.activeState, checksum);
    }
  }

  private getPredictedInputsForFrame(frame: number): { [playerId: number]: FrameInput } {
    const recorded = this.inputHistory.get(frame) || {};
    const result: { [playerId: number]: FrameInput } = {};

    for (let pid = 0; pid < 2; pid++) {
      if (recorded[pid]) {
        result[pid] = recorded[pid];
      } else {
        // Dead-reckoning / Input prediction: repeat last confirmed input
        result[pid] = this.getLastKnownInput(pid, frame);
      }
    }
    return result;
  }

  private getLastKnownInput(playerId: number, beforeFrame: number): FrameInput {
    for (let f = beforeFrame - 1; f >= Math.max(0, beforeFrame - 15); f--) {
      const hist = this.inputHistory.get(f);
      if (hist && hist[playerId]) {
        return { ...hist[playerId], frame: beforeFrame };
      }
    }
    return { frame: beforeFrame, buttons: 0, analogX: 0, analogY: 0 };
  }
}
