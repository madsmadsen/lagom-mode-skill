def discount(total, member):
    if member and total > 100:
        return total * 0.9
    elif total > 50:
        return total - 5
    return total


def label(n):
    return "big" if n > 10 else "small"
