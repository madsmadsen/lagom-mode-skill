package shipping;

public class Route {
    public static String label(int[] legs) {
        StringBuilder out = new StringBuilder();
        int index = 0;
        while (index < legs.length) {
            out.append(legs[index] > 5 ? "long" : "short");
            index++;
        }
        return out.toString();
    }

    public static int one() {
        return 1;
    }
}
