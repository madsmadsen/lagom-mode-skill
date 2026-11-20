import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "src"))

from pricing import discount, label
from util import clamp


class PricingTest(unittest.TestCase):
    def test_member_discount(self):
        self.assertEqual(discount(200, True), 180.0)

    def test_small_total(self):
        self.assertEqual(discount(10, False), 10)

    def test_labels(self):
        self.assertEqual(label(20), "big")


class UtilTest(unittest.TestCase):
    def test_clamp_low(self):
        self.assertEqual(clamp(1, 5, 9), 5)
