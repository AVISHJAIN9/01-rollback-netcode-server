/**
 * Input bitmasks:
 * Bit 0: LEFT
 * Bit 1: RIGHT
 * Bit 2: JUMP
 * Bit 3: ATTACK
 * Bit 4: DASH
 */
export enum InputButtons {
  NONE = 0,
  LEFT = 1 << 0,
  RIGHT = 1 << 1,
  JUMP = 1 << 2,
  ATTACK = 1 << 3,
  DASH = 1 << 4,
}

export interface FrameInput {
  frame: number;
  buttons: number;
  analogX: number; // Q16 raw
  analogY: number; // Q16 raw
}

export interface BundledInputPacket {
  sessionToken: number;
  playerId: number;
  targetFrame: number;
  currentInput: FrameInput;
  historicalInputs: FrameInput[]; // F-1, F-2, F-3, F-4
  stateChecksum: string; // Hash of state at targetFrame - 1
  clientTimestamp: number;
}

export interface ServerAuthoritativePacket {
  serverFrame: number;
  confirmedInputs: { [playerId: number]: FrameInput };
  authoritativeChecksum: string;
  serverTimestamp: number;
  rttEcho: number;
}
