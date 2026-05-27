#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = ["boto3[crt]"]
# ///
"""Tests for stats_report. End-to-end coverage is `make stats` against real logs."""

import gzip
import json
import os
import sys
import unittest
from collections import Counter
from datetime import timedelta

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import stats_report as sr


def _row(
    *,
    ip="1.1.1.1",
    method="GET",
    uri="/",
    status="200",
    referer="-",
    ua="Mozilla/5.0",
    edge="IAD51-P1",
    d="2026-05-20",
):
    """Build a CloudFront log row matching stats_report.COL indexes."""
    r = [""] * 11
    r[sr.COL["date"]] = d
    r[sr.COL["time"]] = "12:00:00"
    r[sr.COL["edge"]] = edge
    r[sr.COL["ip"]] = ip
    r[sr.COL["method"]] = method
    r[sr.COL["uri"]] = uri
    r[sr.COL["status"]] = status
    r[sr.COL["referer"]] = referer
    r[sr.COL["ua"]] = ua
    return r


class ParseWindowTests(unittest.TestCase):
    def test_units(self):
        self.assertEqual(sr.parse_window("7d"), timedelta(days=7))
        self.assertEqual(sr.parse_window("12w"), timedelta(days=84))
        self.assertEqual(sr.parse_window("3mo"), timedelta(days=90))
        self.assertEqual(sr.parse_window("1y"), timedelta(days=365))

    def test_all(self):
        self.assertIsNone(sr.parse_window("all"))

    def test_invalid_raises(self):
        with self.assertRaises(ValueError):
            sr.parse_window("7days")


class WindowAggregateTests(unittest.TestCase):
    def test_bot_row_increments_bot_only(self):
        agg = sr.WindowAggregate()
        agg.record(_row(ua="Googlebot/2.1"))
        self.assertEqual(agg.bot_requests, 1)
        self.assertEqual(agg.pageviews, 0)

    def test_ai_scrapers_classified_as_bots(self):
        # Modern LLM training/search crawlers should land in bot_requests, not pageviews.
        for ua in (
            "Mozilla/5.0 (compatible; GPTBot/1.2; +https://openai.com/gptbot)",
            "Mozilla/5.0 (compatible; ClaudeBot/1.0; +claudebot@anthropic.com)",
            "ChatGPT-User/1.0",
            "Mozilla/5.0 (compatible; OAI-SearchBot/1.0; +https://openai.com/searchbot)",
            "Mozilla/5.0 (compatible; PerplexityBot/1.0)",
            "Mozilla/5.0 (compatible; Bytespider; ByteDance)",
            "Mozilla/5.0 (compatible; Amazonbot/0.1)",
            "Applebot-Extended/1.0",
            "CCBot/2.0 (https://commoncrawl.org/faq/)",
            "meta-externalagent/1.1",
            "anthropic-ai/1.0",
        ):
            agg = sr.WindowAggregate()
            agg.record(_row(ua=ua))
            self.assertEqual(agg.bot_requests, 1, ua)
            self.assertEqual(agg.pageviews, 0, ua)

    def test_non_get_is_dropped(self):
        agg = sr.WindowAggregate()
        agg.record(_row(method="POST"))
        self.assertEqual(agg.pageviews, 0)
        self.assertEqual(agg.bot_requests, 0)

    def test_non_2xx_is_dropped(self):
        agg = sr.WindowAggregate()
        agg.record(_row(status="404"))
        self.assertEqual(agg.pageviews, 0)

    def test_asset_request_is_dropped(self):
        agg = sr.WindowAggregate()
        agg.record(_row(uri="/style.css"))
        self.assertEqual(agg.pageviews, 0)

    def test_pageview_counts(self):
        agg = sr.WindowAggregate()
        agg.record(_row(uri="/"))
        agg.record(_row(uri="/posts/"))
        self.assertEqual(agg.pageviews, 2)
        self.assertEqual(agg.pages["/"], 1)
        self.assertEqual(agg.pages["/posts/"], 1)

    def test_same_site_referrer_collapses_to_direct(self):
        agg = sr.WindowAggregate()
        agg.record(_row(referer="https://tklon.com/posts/"))
        agg.record(_row(referer="https://www.tklon.com/", ip="2.2.2.2"))
        agg.record(_row(referer="-", ip="3.3.3.3"))
        self.assertEqual(agg.referrers["(direct)"], 3)

    def test_lookalike_domain_is_not_direct(self):
        # endswith would falsely match "eviltklon.com"; we want a real subdomain check.
        agg = sr.WindowAggregate()
        agg.record(_row(referer="https://eviltklon.com/"))
        self.assertEqual(agg.referrers["eviltklon.com"], 1)
        self.assertNotIn("(direct)", agg.referrers)

    def test_uri_is_url_decoded(self):
        agg = sr.WindowAggregate()
        agg.record(_row(uri="/posts/hello%20world"))
        self.assertEqual(agg.pages["/posts/hello world"], 1)

    def test_edge_keeps_first_three_chars(self):
        agg = sr.WindowAggregate()
        agg.record(_row(edge="IAD51-P1"))
        self.assertEqual(agg.edges["IAD"], 1)


class ReportAggregateTests(unittest.TestCase):
    def _sample_window(self) -> sr.WindowAggregate:
        agg = sr.WindowAggregate()
        agg.record(_row(uri="/", ip="1.1.1.1"))
        agg.record(_row(uri="/posts/", ip="2.2.2.2"))
        agg.record(_row(uri="/", ip="2.2.2.2"))
        agg.record(_row(ua="bingbot/2.0"))
        return agg

    def test_from_window_copies_fields(self):
        w = self._sample_window()
        r = sr.ReportAggregate.from_window(w)
        self.assertEqual(r.pageviews, 3)
        self.assertEqual(r.bot_requests, 1)

    def test_dict_roundtrip(self):
        r = sr.ReportAggregate.from_window(self._sample_window())
        r2 = sr.ReportAggregate.from_dict(json.loads(json.dumps(r.to_dict())))
        self.assertEqual(r.to_dict(), r2.to_dict())

    def test_merge_sums_counts(self):
        a = sr.ReportAggregate.from_window(self._sample_window())
        b = sr.ReportAggregate.from_window(self._sample_window())
        m = sr.ReportAggregate.merge([a, b])
        self.assertEqual(m.pageviews, a.pageviews * 2)
        self.assertEqual(m.bot_requests, a.bot_requests * 2)
        self.assertEqual(m.pages["/"], a.pages["/"] * 2)

    def test_from_dict_ignores_legacy_visitors_key(self):
        # Older archived weekly JSONs in S3 still carry a `visitors` field;
        # from_dict should silently drop it rather than blow up.
        r = sr.ReportAggregate.from_dict(
            {
                "pageviews": 7,
                "bot_requests": 2,
                "visitors": 99,
                "pages": {"/": 7},
                "referrers": {},
                "edges": {},
            }
        )
        self.assertEqual(r.pageviews, 7)
        self.assertEqual(r.bot_requests, 2)
        self.assertFalse(hasattr(r, "visitors"))


class ParseLogsTests(unittest.TestCase):
    def test_skips_comments_and_short_rows(self):
        lines = [
            "#Version: 1.0",
            "#Fields: date time x-edge-location ...",
            "2026-05-20\t12:00:00",  # too short — no UA column
            "\t".join(_row(uri="/", ua="Mozilla/5.0")),
        ]
        gz = gzip.compress(("\n".join(lines) + "\n").encode())
        rows = list(sr.parse_logs(gz))
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0][sr.COL["uri"]], "/")


class FormatReportTests(unittest.TestCase):
    def _report(self, pv=10, bot=2):
        return sr.ReportAggregate(
            pageviews=pv,
            bot_requests=bot,
            pages=Counter({"/": 7, "/posts/": 3}),
            referrers=Counter({"(direct)": 8, "news.ycombinator.com": 2}),
            edges=Counter({"IAD": 6, "LHR": 4}),
        )

    def test_delta_appears_with_prior(self):
        out = sr.format_report(self._report(pv=20), self._report(pv=10), "test window")
        self.assertIn("PAGEVIEWS", out)
        self.assertIn("+100%", out)

    def test_delta_omitted_when_no_prior(self):
        out = sr.format_report(self._report(), None, "test window")
        self.assertNotIn("vs prior", out)

    def test_edge_city_decoration(self):
        out = sr.format_report(self._report(), None, "test window")
        self.assertIn("IAD (Virginia)", out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
