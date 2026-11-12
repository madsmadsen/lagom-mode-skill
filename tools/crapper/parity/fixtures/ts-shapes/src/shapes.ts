export function plain(x: number): number {
  if (x > 0) { return 1 }
  return 0
}

export var viaVar = (x: number): number => {
  if (x > 0) { return 1 }
  return 0
}

export const store = {
  objectMethod(x: number): number {
    if (x > 0) { return 1 }
    if (x < 0) { return -1 }
    return 0
  },
}

export class Box {
  method(x: number): number {
    if (x > 0) { return 1 }
    return 0
  }
  field = (x: number): number => {
    if (x > 0) { return 1 }
    if (x < 0) { return -1 }
    return 0
  }
}

export var legacy = function (x: number): number {
  if (x > 0) { return 1 }
  return 0
}

export default function (x: number): number {
  if (x > 0) { return 1 }
  return 0
}
