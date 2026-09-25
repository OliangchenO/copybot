#!/usr/bin/env python3
"""Replay the observer's append-only journal using evidence visible at each event."""
import argparse
import collections
import json


def report(path):
    markets = {}
    occupied = collections.Counter()
    stats = {s: collections.Counter() for s in ('BTC', 'ETH')}
    confirmed = {s: set() for s in stats}
    enhanced = {s: set() for s in stats}
    filled = {s: set() for s in stats}
    unknown = {s: set() for s in stats}
    timed_out = {s: set() for s in stats}
    delays = {s: [] for s in stats}
    prices = {s: [] for s in stats}
    with open(path, encoding='utf-8') as handle:
        for line_no, line in enumerate(handle, 1):
            if not line.strip():
                continue
            row = json.loads(line)
            event = row.get('ev')
            condition = row.get('condition')
            if event == 'consensus_candidate':
                symbol = row.get('symbol')
                if symbol not in stats or not isinstance(row.get('trigger_ts'), int):
                    raise ValueError('invalid candidate on line %d' % line_no)
                if row['trigger_ts'] > row['t']:
                    raise ValueError('future candidate on line %d' % line_no)
                if condition not in markets:
                    markets[condition] = row
                    stats[symbol]['candidates'] += 1
                continue
            candidate = markets.get(condition)
            if not candidate:
                continue
            symbol = candidate['symbol']
            tally = stats[symbol]
            if event == 'consensus_decision':
                evidence = row.get('evidence', {})
                if any(isinstance(b, dict) and b.get('as_of', 0) > row['t']
                       for b in evidence.values()):
                    raise ValueError('future wallet snapshot on line %d' % line_no)
                reason = row.get('reason', '')
                if reason.startswith('Target'):
                    tally['confirmed_decisions'] += 1
                    confirmed[symbol].add(condition)
                    if 'total_cents: 1000' in reason:
                        tally['ants_enhanced_decisions'] += 1
                        enhanced[symbol].add(condition)
                elif 'unknown' in reason.lower():
                    tally['unknown_decisions'] += 1
                    unknown[symbol].add(condition)
                elif 'expired' in reason:
                    tally['timeout_decisions'] += 1
                    timed_out[symbol].add(condition)
            elif event == 'consensus_veto':
                tally['vetoes'] += 1
            elif event == 'consensus_quote_skip':
                tally['quote_skips'] += 1
                tally['skip_' + str(row.get('why', 'unspecified'))] += 1
            elif event == 'consensus_sim_fill':
                if row['t'] < candidate['trigger_ts']:
                    raise ValueError('fill before candidate on line %d' % line_no)
                cents = row['cents']
                if not isinstance(cents, int) or cents <= 0:
                    raise ValueError('invalid fill on line %d' % line_no)
                occupied[condition] += cents
                tally['max_occupied_cents'] = max(tally['max_occupied_cents'], sum(
                    amount for market, amount in occupied.items()
                    if markets.get(market, {}).get('symbol') == symbol))
                tally['sim_fills'] += 1
                filled[symbol].add(condition)
                tally['sim_spent_cents'] += cents
                delays[symbol].append(row['t'] - candidate['trigger_ts'])
                prices[symbol].append(row['ask'] / 1_000_000)
            elif event == 'consensus_sim_exit':
                occupied[condition] = 0
                tally['sim_exits'] += 1
                tally['sim_proceeds_cents'] += row.get('proceeds_cents', 0)
    return {'markets': {s: dict(stats[s], confirmed_candidates=len(confirmed[s]),
        confirmation_rate=(len(confirmed[s]) / stats[s]['candidates']
            if stats[s]['candidates'] else None),
        ants_enhanced_candidates=len(enhanced[s]), unknown_candidates=len(unknown[s]),
        simulated_fill_candidates=len(filled[s]),
        simulated_fill_rate=(len(filled[s]) / stats[s]['candidates']
            if stats[s]['candidates'] else None),
        timeout_candidates=len(timed_out[s]),
        avg_fill_delay_seconds=(sum(delays[s]) / len(delays[s])
        if delays[s] else None), avg_fill_price=(sum(prices[s]) / len(prices[s])
        if prices[s] else None)) for s in stats},
        'limits': 'Only observed events are replayed. No final outcome, fees, historical order book, or baseline strategy data is present; net return and baseline comparison are unavailable.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('journal', help='consensus.events_path from the dry config')
    args = parser.parse_args()
    print(json.dumps(report(args.journal), ensure_ascii=False, indent=2, sort_keys=True))
