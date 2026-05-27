#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = ["boto3"]
# ///
"""
Weekly stats report for tklon.com.

Runs in two modes from the same source — single boto3-based code path, no
backend abstraction:

- AWS Lambda (lambda_handler): EventBridge fires weekly, reads the last 7 days
  of CloudFront access logs from S3, publishes a plain-text report to SNS, and
  archives a weekly aggregate JSON to the stats bucket. boto3 is preinstalled
  in the Lambda runtime.
- Local CLI (--local --window …): prints a report for an arbitrary window.
  Invoked via `uv run` (see Makefile), which honours the PEP 723 metadata
  above and provisions boto3 in an isolated, cached venv. No global pip
  install, no system Python pollution, no requirements.txt to drift.

Two aggregate shapes flow through the code:

  WindowAggregate  — built from a contiguous span of raw logs; tracks unique
                     visitor IDs as a set for accurate counting.
  ReportAggregate  — serialised / merged form; visitors collapses to an int.

The split is in the type system on purpose: once a window is serialised,
visitor uniqueness across windows can only be approximated by summing counts.
Making that one-way conversion explicit prevents accidentally treating a
merged aggregate as if it still held full visitor identities.
"""

import argparse
import csv
import gzip
import hashlib
import io
import json
import os
import re
import sys
import urllib.parse
from collections import Counter
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta, timezone

import boto3

# --- Config -----------------------------------------------------------------

LOGS_BUCKET    = os.environ.get("LOGS_BUCKET")
STATS_BUCKET   = os.environ.get("STATS_BUCKET")
SNS_TOPIC_ARN  = os.environ.get("SNS_TOPIC_ARN")
SITE_URL       = os.environ.get("SITE_URL", "https://tklon.com")
LOG_PREFIX     = os.environ.get("LOG_PREFIX", "cloudfront/")
SITE_HOSTNAME  = urllib.parse.urlparse(SITE_URL).hostname or "tklon.com"

# Heuristic bot filter. Refine over time as new bots show up in the logs.
BOT_UA = re.compile(
    r"(googlebot|bingbot|crawl|spider|bot/|headless|curl|wget|monitoring|"
    r"slackbot|facebookexternalhit|twitterbot|linkedinbot|whatsapp|"
    r"telegrambot|pingdom|uptimerobot|ahrefsbot|semrush|mj12bot|duckduckbot)",
    re.IGNORECASE,
)

# Friendly names for the busiest CloudFront edges. Anything not listed is
# shown by its 3-letter IATA code.
EDGE_CITY = {
    "IAD": "Virginia",     "ATL": "Atlanta",       "ORD": "Chicago",
    "DFW": "Dallas",       "JFK": "New York",      "LAX": "Los Angeles",
    "SEA": "Seattle",      "SFO": "San Francisco", "MIA": "Miami",
    "DEN": "Denver",       "YUL": "Montreal",      "YYZ": "Toronto",
    "LHR": "London",       "DUB": "Dublin",        "CDG": "Paris",
    "FRA": "Frankfurt",    "AMS": "Amsterdam",     "ARN": "Stockholm",
    "NRT": "Tokyo",        "ICN": "Seoul",         "SIN": "Singapore",
    "SYD": "Sydney",       "GRU": "São Paulo",     "MEX": "Mexico City",
}

# CloudFront standard log columns (tab-separated). Order matters — full
# reference: https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/AccessLogs.html#access-logs-file-format
# We only index by name for the fields we use.
COL = {
    "date": 0, "time": 1, "edge": 2, "ip": 4, "method": 5,
    "uri": 7, "status": 8, "referer": 9, "ua": 10,
}


# --- Aggregate types --------------------------------------------------------

@dataclass
class WindowAggregate:
    """Live aggregate built from a contiguous span of raw log rows.

    `visitors` is a set of hashed (IP, UA, day) tuples so uniqueness is
    accurate within the window. Convert to ReportAggregate before serialising
    or merging across windows.
    """
    pageviews:    int                  = 0
    bot_requests: int                  = 0
    visitors:     set[str]             = field(default_factory=set)
    pages:        Counter[str]         = field(default_factory=Counter)
    referrers:    Counter[str]         = field(default_factory=Counter)
    edges:        Counter[str]         = field(default_factory=Counter)

    def record(self, row: list[str]) -> None:
        """Fold one log row into this aggregate."""
        ua = row[COL["ua"]]
        if BOT_UA.search(ua):
            self.bot_requests += 1
            return
        if row[COL["method"]] != "GET":
            return
        uri = urllib.parse.unquote(row[COL["uri"]])
        status = row[COL["status"]]
        if not (status.startswith("2") and _is_html_path(uri)):
            return

        self.pageviews += 1
        self.pages[uri] += 1

        # Approximate unique visitor: SHA-256 of (IP, UA, day-salt). Day-rotating
        # salt means a visitor can't be tracked across days from the aggregate.
        salt = row[COL["date"]]
        vid = hashlib.sha256(f"{row[COL['ip']]}|{ua}|{salt}".encode()).hexdigest()[:16]
        self.visitors.add(vid)

        ref = row[COL["referer"]]
        if ref and ref != "-":
            host = urllib.parse.urlparse(ref).hostname or ""
            # Treat same-site referrers as direct: we only want external sources.
            host = host if host and not host.endswith(SITE_HOSTNAME) else "(direct)"
        else:
            host = "(direct)"
        self.referrers[host] += 1

        edge = row[COL["edge"]][:3]
        if edge:
            self.edges[edge] += 1


@dataclass
class ReportAggregate:
    """Serialisable, mergeable aggregate. `visitors` is a count, not a set."""
    pageviews:    int                  = 0
    bot_requests: int                  = 0
    visitors:     int                  = 0
    pages:        Counter[str]         = field(default_factory=Counter)
    referrers:    Counter[str]         = field(default_factory=Counter)
    edges:        Counter[str]         = field(default_factory=Counter)

    @classmethod
    def from_window(cls, w: WindowAggregate) -> "ReportAggregate":
        return cls(
            pageviews    = w.pageviews,
            bot_requests = w.bot_requests,
            visitors     = len(w.visitors),
            pages        = w.pages,
            referrers    = w.referrers,
            edges        = w.edges,
        )

    @classmethod
    def from_dict(cls, d: dict) -> "ReportAggregate":
        """Hydrate from a JSON-serialised weekly aggregate."""
        return cls(
            pageviews    = d.get("pageviews", 0),
            bot_requests = d.get("bot_requests", 0),
            visitors     = d.get("visitors", 0),
            pages        = Counter(d.get("pages", {})),
            referrers    = Counter(d.get("referrers", {})),
            edges        = Counter(d.get("edges", {})),
        )

    @classmethod
    def merge(cls, parts: "list[ReportAggregate]") -> "ReportAggregate":
        """Combine many reports. Visitor count sums approximately — see the
        module docstring for why merging can't preserve true uniqueness."""
        out = cls()
        for a in parts:
            out.pageviews    += a.pageviews
            out.bot_requests += a.bot_requests
            out.visitors     += a.visitors
            out.pages.update(a.pages)
            out.referrers.update(a.referrers)
            out.edges.update(a.edges)
        return out

    def to_dict(self) -> dict:
        return {
            "pageviews":    self.pageviews,
            "bot_requests": self.bot_requests,
            "visitors":     self.visitors,
            "pages":        dict(self.pages),
            "referrers":    dict(self.referrers),
            "edges":        dict(self.edges),
        }


# --- Parsing ----------------------------------------------------------------

def _is_html_path(uri: str) -> bool:
    """Treat directory-style URIs and *.html as pageviews; everything else is an asset."""
    if uri.endswith("/") or uri.endswith(".html"):
        return True
    last = uri.rsplit("/", 1)[-1]
    return "." not in last  # extensionless paths are HTML (CF function adds index.html)


def parse_logs(gz_bytes: bytes):
    """Yield row lists from a single gzipped CloudFront log file."""
    with gzip.GzipFile(fileobj=io.BytesIO(gz_bytes)) as gz:
        text = io.TextIOWrapper(gz, encoding="utf-8", errors="replace")
        for row in csv.reader(text, delimiter="\t"):
            if not row or row[0].startswith("#"):
                continue
            if len(row) > COL["ua"]:
                yield row


# --- S3 plumbing ------------------------------------------------------------

def aggregate_raw_logs(s3, start_date: date, end_date: date) -> WindowAggregate:
    """Read all CF logs whose filename date falls in [start, end] and aggregate."""
    agg = WindowAggregate()
    key_date = re.compile(r"(\d{4})-(\d{2})-(\d{2})-\d{2}\.[\w-]+\.gz$")
    for page in s3.get_paginator("list_objects_v2").paginate(
        Bucket=LOGS_BUCKET, Prefix=LOG_PREFIX
    ):
        for obj in page.get("Contents", []):
            m = key_date.search(obj["Key"])
            if not m:
                continue
            d = date(int(m[1]), int(m[2]), int(m[3]))
            if not (start_date <= d <= end_date):
                continue
            body = s3.get_object(Bucket=LOGS_BUCKET, Key=obj["Key"])["Body"].read()
            for row in parse_logs(body):
                agg.record(row)
    return agg


def load_weekly_aggregates(s3, start_date: date, end_date: date):
    """Yield ReportAggregate per weekly JSON whose ISO week overlaps [start, end]."""
    key_re = re.compile(r"weekly/(\d{4})-W(\d{2})\.json$")
    for page in s3.get_paginator("list_objects_v2").paginate(
        Bucket=STATS_BUCKET, Prefix="weekly/"
    ):
        for obj in page.get("Contents", []):
            m = key_re.match(obj["Key"])
            if not m:
                continue
            week_start = date.fromisocalendar(int(m[1]), int(m[2]), 1)
            week_end = week_start + timedelta(days=6)
            if week_end < start_date or week_start > end_date:
                continue
            body = s3.get_object(Bucket=STATS_BUCKET, Key=obj["Key"])["Body"].read()
            yield ReportAggregate.from_dict(json.loads(body))


# --- Report formatting ------------------------------------------------------

def _delta(cur: int, prev: int) -> str:
    if not prev:
        return ""
    pct = (cur - prev) / prev * 100
    sign = "+" if pct >= 0 else ""
    return f"  ({sign}{pct:.0f}% vs prior)"


def format_report(cur: ReportAggregate, prev: ReportAggregate | None, label: str) -> str:
    """Build the plain-text report body."""
    prev_pv = prev.pageviews if prev else 0
    out = [
        f"tklon.com — {label}",
        "",
        f"PAGEVIEWS    {cur.pageviews:>5d}{_delta(cur.pageviews, prev_pv)}",
        f"VISITORS     {cur.visitors:>5d}   (unique, approx)",
        f"BOT TRAFFIC  {cur.bot_requests:>5d}   (filtered)",
        "",
    ]

    def top_section(title: str, counter: Counter, decorate=lambda k: k):
        out.append(title)
        for k, n in counter.most_common(10):
            out.append(f"  {n:>5d}  {decorate(k)}")
        out.append("")

    top_section("TOP PAGES", cur.pages)
    top_section("TOP REFERRERS", cur.referrers)
    top_section(
        "TOP EDGE LOCATIONS",
        cur.edges,
        lambda code: f"{code} ({EDGE_CITY[code]})" if code in EDGE_CITY else code,
    )
    return "\n".join(out).rstrip() + "\n"


# --- Window math ------------------------------------------------------------

WINDOW_RE = re.compile(r"^(\d+)(d|w|mo|y)$")
UNIT_DAYS = {"d": 1, "w": 7, "mo": 30, "y": 365}


def parse_window(s: str) -> timedelta | None:
    """'7d' → timedelta(7); 'all' → None (open-ended)."""
    if s == "all":
        return None
    m = WINDOW_RE.match(s)
    if not m:
        raise ValueError(f"Invalid window {s!r}: use e.g. 7d, 12w, 3mo, 1y, or 'all'")
    return timedelta(days=int(m[1]) * UNIT_DAYS[m[2]])


# --- Entry points -----------------------------------------------------------

def lambda_handler(event, context):
    """EventBridge → Lambda. Last 7d → SNS email + S3 archive."""
    s3 = boto3.client("s3")
    sns = boto3.client("sns")

    end = datetime.now(timezone.utc).date()
    start = end - timedelta(days=7)
    cur  = ReportAggregate.from_window(aggregate_raw_logs(s3, start, end))
    prev = ReportAggregate.from_window(
        aggregate_raw_logs(s3, start - timedelta(days=7), start - timedelta(days=1))
    )

    label = f"week ending {end.isoformat()}"
    body = format_report(cur, prev, label)

    sns.publish(
        TopicArn=SNS_TOPIC_ARN,
        Subject=f"tklon.com — {label}",
        Message=body,
    )

    iso_year, iso_week, _ = end.isocalendar()
    key = f"weekly/{iso_year}-W{iso_week:02d}.json"
    s3.put_object(
        Bucket=STATS_BUCKET, Key=key,
        Body=json.dumps(cur.to_dict(), indent=2).encode(),
        ContentType="application/json",
    )
    return {"status": "ok", "archived": key, "pageviews": cur.pageviews}


def cli(argv):
    """`make stats` entry point. Window is parsed, raw logs and/or weekly aggregates are stitched."""
    p = argparse.ArgumentParser()
    p.add_argument("--window", default="7d", help="e.g. 7d, 12w, 3mo, 1y, or 'all' (default: 7d)")
    p.add_argument("--local", action="store_true", help="Required outside Lambda")
    args = p.parse_args(argv)
    if not args.local:
        p.error("--local is required when running as a CLI")

    s3 = boto3.client("s3")
    window = parse_window(args.window)
    end = datetime.now(timezone.utc).date()
    raw_horizon = end - timedelta(days=90)  # CF logs live 90d

    cur: ReportAggregate
    prev: ReportAggregate | None

    if window is None or end - window < raw_horizon:
        # Long window: stitch stored aggregates + current-partial-week raw logs.
        start = date(2020, 1, 1) if window is None else end - window
        parts = list(load_weekly_aggregates(s3, start, end))
        parts.append(ReportAggregate.from_window(
            aggregate_raw_logs(s3, max(start, raw_horizon), end)
        ))
        cur = ReportAggregate.merge(parts)
        prev = None  # no comparison for long windows
    else:
        # Short window: raw logs only, with prior-period comparison.
        start = end - window
        cur = ReportAggregate.from_window(aggregate_raw_logs(s3, start, end))
        prev_end = start - timedelta(days=1)
        prev_start = prev_end - window + timedelta(days=1)
        prev = ReportAggregate.from_window(aggregate_raw_logs(s3, prev_start, prev_end))

    label = "all time" if window is None else f"last {args.window}"
    print(format_report(cur, prev, label))


if __name__ == "__main__":
    cli(sys.argv[1:])
