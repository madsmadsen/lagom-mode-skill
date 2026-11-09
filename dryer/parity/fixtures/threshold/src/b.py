def total_b(orders):
    total = 0
    count = 0
    for order in orders:
        if order.paid and order.domestic:
            count = count + 1
            total = total + order.amount
    tax = total / 10
    fee = count + tax
    total = total + 1
    return total + tax + count
