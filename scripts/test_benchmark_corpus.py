"""Check comparison error classification and complete-set pagination."""
import unittest
from unittest.mock import patch

from benchmark_corpus import known_language_rejection, snowstorm


class ComparisonTests(unittest.TestCase):
    def test_only_confirmed_extrema_parser_rejections_are_unsupported(self):
        body = "Syntax error at line 1, character 0: mismatched input '!' expecting '('"
        self.assertTrue(known_language_rejection("!!> (1000001 OR 1000002)", 400, body))
        self.assertTrue(known_language_rejection("!!< (1000001 OR 1000002)", 400, body))
        self.assertFalse(known_language_rejection("!!> 1000001", 500, body))
        self.assertFalse(known_language_rejection("!!> 1000001", 400, "Invalid branch"))
        self.assertFalse(known_language_rejection("< 1000001", 400, body))

    @patch("benchmark_corpus.http")
    def test_all_pages_are_compared(self, http):
        http.side_effect = [
            {"total": 3, "items": ["1000001", "1000002"], "searchAfter": "cursor"},
            {"total": 3, "items": ["1000003"]},
        ]
        self.assertEqual(snowstorm("http://localhost", "*"), {"1000001", "1000002", "1000003"})
        self.assertEqual(http.call_args.args[2]["searchAfter"], "cursor")

    @patch("benchmark_corpus.http")
    def test_duplicate_or_incomplete_pages_fail(self, http):
        for pages in [
            [{"total": 2, "items": ["1000001", "1000001"]}],
            [{"total": 2, "items": ["1000001"]}],
            [{"total": 2, "items": ["1000001"], "searchAfter": "cursor"},
             {"total": 2, "items": ["1000001"], "searchAfter": "cursor2"}],
        ]:
            http.side_effect = pages
            with self.assertRaises(ValueError):
                snowstorm("http://localhost", "*")


if __name__ == "__main__":
    unittest.main()
