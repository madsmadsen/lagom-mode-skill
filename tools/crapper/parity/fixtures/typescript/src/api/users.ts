import express from "express";

export const router = express.Router();

export function userName(user: { name?: string } | null): string {
  return user?.name ?? "anonymous";
}

export function pick(items: number[], flag: boolean): number {
  if (!flag) {
    return 0;
  }
  return items.length > 0 ? items[0] : -1;
}

router.get("/users", (req, res) => {
  res.json([]);
});

export class Store {
  find(id: string): string | undefined {
    for (const key of ["a", "b"]) {
      if (key === id) {
        return key;
      }
    }
    return undefined;
  }
}
