"""Check comparison error classification and complete-set pagination."""
import unittest
import hashlib
import json
from copy import deepcopy
from unittest.mock import patch

from benchmark_corpus import known_language_rejection, snowstorm
from summarise_corpus import compare_prior


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


class CorpusExtensionTests(unittest.TestCase):
    def setUp(self):
        self.old_case = {'id': 'a', 'category': 'literal', 'ecl': '100001000'}
        self.new_case = {'id': 'b', 'category': 'hierarchy', 'ecl': '<<100001000'}
        self.old = json.dumps({'cases': [self.old_case]}).encode()
        self.new = json.dumps({'cases': [self.old_case, self.new_case]}).encode()
        self.prior = {'edition': 'synthetic', 'archive_sha256': 'archive',
                      'corpus_sha256': hashlib.sha256(self.old).hexdigest(),
                      'result_digests': [{'id': 'a', 'status': 'evaluated', 'total': 1, 'sha256': 'set-a'}]}
        self.report = {'edition': 'synthetic', 'archive_sha256': 'archive',
                       'corpus_sha256': hashlib.sha256(self.new).hexdigest(),
                       'results': [dict(self.old_case, status='evaluated', total=1, sha256='set-a'),
                                   dict(self.new_case, status='evaluated', total=2, sha256='set-b')]}

    def test_extension_preserves_old_expressions_and_complete_sets(self):
        self.assertEqual(compare_prior(self.report, self.prior, self.new, self.old), 1)

    def test_changed_corpus_requires_pinned_documents(self):
        with self.assertRaises(ValueError):
            compare_prior(self.report, self.prior)
        with self.assertRaises(ValueError):
            compare_prior(self.report, self.prior, self.new + b' ', self.old)

    def test_same_count_with_different_members_fails(self):
        self.report['results'][0]['sha256'] = 'different-set'
        with self.assertRaisesRegex(ValueError, 'Changed complete result set'):
            compare_prior(self.report, self.prior, self.new, self.old)

    def test_reusing_an_id_for_a_changed_expression_fails(self):
        changed = dict(self.old_case, ecl='100002000')
        document = json.dumps({'cases': [changed, self.new_case]}).encode()
        self.report['corpus_sha256'] = hashlib.sha256(document).hexdigest()
        with self.assertRaisesRegex(ValueError, 'Changed or missing regression expression'):
            compare_prior(self.report, self.prior, document, self.old)

    def test_incomplete_or_wrong_expression_report_fails(self):
        for mutation in ('missing', 'expression', 'duplicate'):
            report = deepcopy(self.report)
            if mutation == 'missing':
                report['results'].pop()
            elif mutation == 'expression':
                report['results'][1]['ecl'] = '*'
            else:
                report['results'].append(report['results'][0])
            with self.assertRaises(ValueError):
                compare_prior(report, self.prior, self.new, self.old)


if __name__ == "__main__":
    unittest.main()
