"""Module docstring: the tokens and, or, not, True, 0, and 1 stay put here."""

NOTE = "a and b or not c True 0 1 == != < + *"


class Board:
    def __init__(self) -> None:
        self.cells = 0

    def place(self, row: int, col: int) -> bool:
        return row >= 0 and col < 8 or not self._full()

    def _full(self) -> bool:
        return self.cells == 64 or False

    def score(self, bonus: int) -> int:
        if bonus < 0:
            return -bonus * 2
        return self.cells + bonus * 1 - 0


def _hide(flag: bool) -> bool:
    return flag == True and 1 != 0


def open_square(n: int) -> int:
    return n * 2 + 1
