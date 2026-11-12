package shipping;

import java.util.Comparator;

abstract class Shape {
    private final int sides;

    Shape(int sides) {
        this.sides = sides;
    }

    abstract int area();

    Comparator<Shape> byArea() {
        return new Comparator<Shape>() {
            public int compare(Shape a, Shape b) {
                return a.area() - b.area();
            }
        };
    }

    int twice() {
        class Helper {
            int go(int x) {
                return x * 2;
            }
        }
        return new Helper().go(area());
    }
}
