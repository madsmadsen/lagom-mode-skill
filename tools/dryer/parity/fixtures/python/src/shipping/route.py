def route_label(legs):
    out = ""
    index = 0
    while index < len(legs):
        out = out + ("long" if legs[index] > 5 else "short")
        index += 1
    return out


def one():
    return 1
