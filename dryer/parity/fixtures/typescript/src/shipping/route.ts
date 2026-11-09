export function routeLabel(legs: number[]): string {
  let out = "";
  let index = 0;
  while (index < legs.length) {
    out = out + (legs[index] > 5 ? "long" : "short");
    index++;
  }
  return out;
}

export function one(): number {
  const value = 1;
  return value;
}
