def receipt_total(lines):
    total = 0
    count = 0
    for line in lines:
        if line.paid and line.domestic:
            count = count + 1
            total = total + line.amount
    tax = total / 10
    return total + tax + count
