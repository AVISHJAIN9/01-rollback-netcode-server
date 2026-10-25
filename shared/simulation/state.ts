import { FixedI32, Vec2Fixed } from "../math/fixed_point";
import { PhysicsEngine } from "../physics/aabb";
import { FrameInput, InputButtons } from "../protocol/packets";

/**
 * Deterministic PCG32 Random Number Generator
 */
export class DeterministicRNG {
  private state: bigint;
  private inc: bigint;

  constructor(seed: bigint, seq: bigint = 1n) {
    this.state = 0n;
    this.inc = (seq << 1n) | 1n;
    this.next();
    this.state += seed;
    this.next();
  }

  public next(): number {
    const oldstate = this.state;
    this.state = (oldstate * 6364136223846793005n + this.inc) & 0xFFFFFFFFFFFFFFFFn;
    const xorshifted = Number(((oldstate >> 18n) ^ oldstate) >> 27n) & 0xFFFFFFFF;
    const rot = Number(oldstate >> 59n);
    return ((xorshifted >>> rot) | (xorshifted << ((-rot) & 31))) >>> 0;
  }

  public nextFloat(): number {
    return this.next() / 4294967296.0;
  }
}

export interface PlayerEntity {
  id: number;
  position: Vec2Fixed;
  velocity: Vec2Fixed;
  health: number;
  facingLeft: boolean;
  isGrounded: boolean;
  isAttacking: boolean;
  attackCooldown: number;
}

export class WorldState {
  public frameIndex: number;
  public players: PlayerEntity[];
  public rngSeed: bigint;

  constructor(frameIndex: number = 0, rngSeed: bigint = 42n) {
    this.frameIndex = frameIndex;
    this.rngSeed = rngSeed;
    this.players = [
      {
        id: 0,
        position: Vec2Fixed.fromFloats(200, 400),
        velocity: Vec2Fixed.fromFloats(0, 0),
        health: 100,
        facingLeft: false,
        isGrounded: true,
        isAttacking: false,
        attackCooldown: 0,
      },
      {
        id: 1,
        position: Vec2Fixed.fromFloats(600, 400),
        velocity: Vec2Fixed.fromFloats(0, 0),
        health: 100,
        facingLeft: true,
        isGrounded: true,
        isAttacking: false,
        attackCooldown: 0,
      },
    ];
  }

  public clone(): WorldState {
    const clone = new WorldState(this.frameIndex, this.rngSeed);
    clone.players = this.players.map((p) => ({
      id: p.id,
      position: p.position.clone(),
      velocity: p.velocity.clone(),
      health: p.health,
      facingLeft: p.facingLeft,
      isGrounded: p.isGrounded,
      isAttacking: p.isAttacking,
      attackCooldown: p.attackCooldown,
    }));
    return clone;
  }

  public stepPhysics(inputs: { [playerId: number]: FrameInput }): void {
    const SPEED = FixedI32.fromFloat(3.2);
    const JUMP_FORCE = FixedI32.fromFloat(-9.5);

    for (const player of this.players) {
      const input = inputs[player.id] || { buttons: 0, analogX: 0, analogY: 0, frame: this.frameIndex };

      // Horizontal movement
      if (input.buttons & InputButtons.LEFT) {
        player.velocity.x = SPEED.mul(FixedI32.fromFloat(-1.0));
        player.facingLeft = true;
      } else if (input.buttons & InputButtons.RIGHT) {
        player.velocity.x = SPEED;
        player.facingLeft = false;
      } else {
        player.velocity.x = player.velocity.x.mul(PhysicsEngine.FRICTION);
        if (player.velocity.x.abs().raw < FixedI32.fromFloat(0.05).raw) {
          player.velocity.x = FixedI32.ZERO;
        }
      }

      // Jump
      if (input.buttons & InputButtons.JUMP && player.isGrounded) {
        player.velocity.y = JUMP_FORCE;
        player.isGrounded = false;
      }

      // Gravity
      if (!player.isGrounded) {
        player.velocity.y = player.velocity.y.add(PhysicsEngine.GRAVITY);
      }

      // Apply kinematics
      player.position = player.position.add(player.velocity);

      // Floor collision
      if (player.position.y.raw >= PhysicsEngine.FLOOR_Y.raw) {
        player.position.y = PhysicsEngine.FLOOR_Y;
        player.velocity.y = FixedI32.ZERO;
        player.isGrounded = true;
      }

      // Arena walls
      const MIN_X = FixedI32.fromFloat(20.0);
      const MAX_X = FixedI32.fromFloat(780.0);
      if (player.position.x.raw < MIN_X.raw) player.position.x = MIN_X;
      if (player.position.x.raw > MAX_X.raw) player.position.x = MAX_X;

      // Attack cooldown
      if (player.attackCooldown > 0) {
        player.attackCooldown--;
      }
      player.isAttacking = (input.buttons & InputButtons.ATTACK) !== 0 && player.attackCooldown === 0;
      if (player.isAttacking) {
        player.attackCooldown = 15; // 15 frames cooldown
      }
    }

    // Check combat hitboxes
    for (let i = 0; i < this.players.length; i++) {
      const attacker = this.players[i];
      if (attacker.isAttacking) {
        for (let j = 0; j < this.players.length; j++) {
          if (i === j) continue;
          const target = this.players[j];
          const dist = attacker.position.x.sub(target.position.x).abs();
          if (dist.raw < FixedI32.fromFloat(48.0).raw && Math.abs(attacker.position.y.sub(target.position.y).raw) < FixedI32.fromFloat(32.0).raw) {
            target.health = Math.max(0, target.health - 8);
            target.velocity.x = attacker.facingLeft ? FixedI32.fromFloat(-6.0) : FixedI32.fromFloat(6.0);
            target.velocity.y = FixedI32.fromFloat(-3.0);
            target.isGrounded = false;
          }
        }
      }
    }

    this.frameIndex++;
  }

  /**
   * Continuous Canonical Checksum for Desync Detection
   */
  public computeChecksum(): string {
    let hash = 0x811c9dc5; // FNV-1a 32-bit hash baseline
    const push = (val: number) => {
      hash ^= val & 0xff;
      hash = Math.imul(hash, 0x01000193);
      hash ^= (val >> 8) & 0xff;
      hash = Math.imul(hash, 0x01000193);
      hash ^= (val >> 16) & 0xff;
      hash = Math.imul(hash, 0x01000193);
      hash ^= (val >> 24) & 0xff;
      hash = Math.imul(hash, 0x01000193);
    };

    push(this.frameIndex);
    for (const p of this.players) {
      push(p.id);
      push(p.position.x.raw);
      push(p.position.y.raw);
      push(p.velocity.x.raw);
      push(p.velocity.y.raw);
      push(p.health);
      push(p.isGrounded ? 1 : 0);
      push(p.facingLeft ? 1 : 0);
    }
    return (hash >>> 0).toString(16).padStart(8, "0").toUpperCase();
  }
}
