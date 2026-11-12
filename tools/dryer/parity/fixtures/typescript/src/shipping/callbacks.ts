export const scale = (values: number[]): number[] => {
  const out: number[] = [];
  for (const value of values) {
    out.push(value * 2);
  }
  return out;
};

export class Ledger {
  total(values: number[]): number {
    const total = values.reduce((acc, value) => acc + value, 0);
    return total;
  }
}

export function sumAll(values: number[]): number {
  const twice = (value: number): number => {
    const doubled = value * 2;
    return doubled;
  };
  return values.map((value) => twice(value)).reduce((a, b) => a + b, 0);
}
