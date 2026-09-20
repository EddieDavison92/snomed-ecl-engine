"""Check the independent RF2 reference traversal on an overlapping hierarchy."""
import unittest

from check_corpus_rf2 import expected


class ReferenceTests(unittest.TestCase):
    def setUp(self):
        self.parents = {2: {1}, 3: {1}, 4: {2, 3}, 5: {3}}
        self.children = {1: {2, 3}, 2: {4}, 3: {4, 5}}

    def expand(self, category, *ids):
        return expected(category, ids, self.parents, self.children, {7: {4, 5}})

    def test_diamond_and_self_boundaries(self):
        self.assertEqual(self.expand('descendants', 1), {1, 2, 3, 4, 5})
        self.assertEqual(self.expand('strict-descendants', 1), {2, 3, 4, 5})
        self.assertEqual(self.expand('ancestors', 4), {1, 2, 3})
        self.assertEqual(self.expand('ancestors-or-self', 4), {1, 2, 3, 4})
        self.assertEqual(self.expand('parents-or-self', 4), {2, 3, 4})
        self.assertEqual(self.expand('children-or-self', 4), {4})
        self.assertEqual(self.expand('nested-hierarchy', 2), {2, 3})

    def test_overlapping_sets_and_extrema(self):
        self.assertEqual(self.expand('union', 2, 3), {2, 3, 4, 5})
        self.assertEqual(self.expand('intersection', 2, 3), {4})
        self.assertEqual(self.expand('exclusion', 3, 2), {3, 5})
        self.assertEqual(self.expand('top', 1, 4), {1})
        self.assertEqual(self.expand('bottom', 1, 4), {4})
        self.assertEqual(self.expand('top', 2, 3), {2, 3})

    def test_projection_absence_is_not_an_identity_mapping(self):
        self.assertEqual(self.expand('member-projection', 7), {4, 5})
        self.assertEqual(self.expand('member-projection', 6), set())


if __name__ == '__main__':
    unittest.main()
