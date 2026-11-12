package demo.pkg;

public class Board {
    public int place(int x, int y) {
        if (x < 0 || y < 0) {
            return -1;
        }
        for (int i = 0; i < x; i++) {
            y += i;
        }
        return y > 10 ? y : 10;
    }

    public String name(int k) {
        switch (k) {
            case 1:
                return "one";
            case 2:
                return "two";
            default:
                return "many";
        }
    }
}
