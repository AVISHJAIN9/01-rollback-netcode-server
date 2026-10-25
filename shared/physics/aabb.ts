import { FixedI32, Vec2Fixed } from "../math/fixed_point";

export interface AABB {
  min: Vec2Fixed;
  max: Vec2Fixed;
}

export class PhysicsEngine {
  public static readonly GRAVITY = FixedI32.fromFloat(0.45);
  public static readonly FRICTION = FixedI32.fromFloat(0.85);
  public static readonly MAX_VELOCITY = FixedI32.fromFloat(12.0);
  public static readonly ARENA_WIDTH = FixedI32.fromFloat(800.0);
  public static readonly ARENA_HEIGHT = FixedI32.fromFloat(500.0);
  public static readonly FLOOR_Y = FixedI32.fromFloat(420.0);

  public static checkCollision(a: AABB, b: AABB): boolean {
    return (
      a.min.x.raw < b.max.x.raw &&
      a.max.x.raw > b.min.x.raw &&
      a.min.y.raw < b.max.y.raw &&
      a.max.y.raw > b.min.y.raw
    );
  }

  public static getEntityAABB(pos: Vec2Fixed, halfWidth: number = 16, halfHeight: number = 24): AABB {
    const hw = FixedI32.fromFloat(halfWidth);
    const hh = FixedI32.fromFloat(halfHeight);
    return {
      min: new Vec2Fixed(pos.x.sub(hw), pos.y.sub(hh)),
      max: new Vec2Fixed(pos.x.add(hw), pos.y.add(hh)),
    };
  }
}
