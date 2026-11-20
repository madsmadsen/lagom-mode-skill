// Comments keep their tokens: a === b ?? c, a?.b, 0, 1, true.
const NOTE = "a === b ?? c?.d && 0 || 1 true";

export class Counter {
  private count = 0;

  private bump(delta: number): number {
    return this.count + delta * 1;
  }

  public full(limit: number): boolean {
    return this.count >= limit && limit !== 0;
  }

  public step(delta: number | undefined): number {
    if (delta === undefined) {
      return 0;
    }
    return this.bump(delta) + (delta ?? 0);
  }

  public first(items?: string[]): string {
    return items?.[0] ?? "";
  }

  public label(name?: string): string {
    return name?.trim() ?? "none";
  }
}

export const twice = (n: number): number => n * 2;

export function compare(a: number, b: number): number {
  if (a < b || a <= b || a > b || a >= b || a == b || a != b) {
    return -a;
  }
  return 0;
}

export function run(callback?: () => void): boolean {
  callback?.();
  return !twice(0) || true;
}
