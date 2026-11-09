class Shape:
    def __init__(self, width, height):
        self.width = width
        self.height = height

    def perimeter(self):
        w = self.width
        h = self.height
        return 2 * (w + h)

    def scaled(self, factor):
        def grow(value):
            return value * factor
        return [grow(self.width), grow(self.height)]
