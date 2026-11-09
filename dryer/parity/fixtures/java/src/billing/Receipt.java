package billing;

import java.util.List;

public class Receipt {
    public static long total(List<Order> orders) {
        long sum = 0;
        int count = 0;
        for (Order order : orders) {
            if (order.isPaid() && order.isDomestic()) {
                count = count + 1;
                sum = sum + order.amount();
            }
        }
        long tax = sum / 10;
        long fee = count * 2;
        return sum + tax + count + fee;
    }
}
