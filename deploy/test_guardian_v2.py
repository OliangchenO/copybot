import unittest
from unittest.mock import patch

import guardian


class V2PositionTests(unittest.TestCase):
    def test_complete_cursor_walk_preserves_legacy_position_fields(self):
        pages = [
            {'data': [{'token_id': '1', 'current_size': 2, 'avg_price': 0.4,
                       'current_value': 0.8}],
             'pagination': {'has_more': True, 'next_cursor': 'next'}},
            {'data': [{'token_id': '2', 'current_size': 3, 'avg_price': 0.5,
                       'current_value': 1.5}],
             'pagination': {'has_more': False, 'next_cursor': None}},
        ]
        with patch.object(guardian, 'http_json', side_effect=pages) as fetch:
            positions, complete, _ = guardian.fetch_all_positions('0x' + '1' * 40)
        self.assertTrue(complete)
        self.assertEqual(set(positions), {'1', '2'})
        self.assertEqual(positions['1']['size'], 2)
        self.assertEqual(positions['2']['avgPrice'], 0.5)
        self.assertIn('cursor=next', fetch.call_args.args[0])

    def test_missing_page_cannot_prove_an_empty_wallet(self):
        with patch.object(guardian, 'http_json', return_value={'data': []}):
            positions, complete, _ = guardian.fetch_all_positions('0x' + '1' * 40)
        self.assertEqual(positions, {})
        self.assertFalse(complete)


if __name__ == '__main__':
    unittest.main()
