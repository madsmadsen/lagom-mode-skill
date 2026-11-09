package demo;

// Comments keep their tokens: a == b && c || d, 1 + 2, true, 0.
public class Counter {
    private static final String NOTE = "a == b && 1 + 2 < 3 true";
    private int count = 0;

    public boolean isFull(int limit) {
        return count >= limit && limit != 0;
    }

    public int step(int delta) {
        if (delta < 0) {
            return -delta * 2;
        }
        return count + delta / 1;
    }

    private boolean ready() {
        return !isFull(3) || true;
    }

    public static class Pair {
        int left = 1;
        int right = 0;

        boolean flip(boolean b) {
            return b == false;
        }
    }
}
