#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
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

parse_logs(), update_aggregate(), and format_report() are shared — there is
one definition of "what's in the report" and "how a pageview is counted."
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
import urllib.request
from collections import Counter
from datetime import date, datetime, timedelta, timezone

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

# Permalink format from src/config.rb: /posts/{title}-{day}{month}{year}/
POST_URL = re.compile(r"/posts/([a-z0-9-]+)-(\d{2})(\d{2})(\d{4})/")

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


# --- Parsing & aggregation --------------------------------------------------

def _is_html_path(uri):
    """Treat directory-style URIs and *.html as pageviews; everything else is an asset."""
    if uri.endswith("/") or uri.endswith(".html"):
        return True
    last = uri.rsplit("/", 1)[-1]
    return "." not in last  # extensionless paths are HTML (CF function adds index.html)


def parse_logs(gz_bytes):
    """Yield row lists from a single gzipped CloudFront log file."""
    with gzip.GzipFile(fileobj=io.BytesIO(gz_bytes)) as gz:
        text = io.TextIOWrapper(gz, encoding="utf-8", errors="replace")
        for row in csv.reader(text, delimiter="\t"):
            if not row or row[0].startswith("#"):
                continue
            if len(row) > COL["ua"]:
                yield row


def empty_aggregate():
    return {
        "pageviews": 0,
        "bot_requests": 0,
        "visitors": set(),
        "pages": Counter(),
        "referrers": Counter(),
        "edges": Counter(),
    }


def update_aggregate(agg, row):
    """Fold one log row into the running aggregate."""
    ua = row[COL["ua"]]
    if BOT_UA.search(ua):
        agg["bot_requests"] += 1
        return
    if row[COL["method"]] != "GET":
        return
    uri = urllib.parse.unquote(row[COL["uri"]])
    status = row[COL["status"]]
    if not (status.startswith("2") and _is_html_path(uri)):
        return

    agg["pageviews"] += 1
    agg["pages"][uri] += 1

    # Approximate unique visitor: SHA-256 of (IP, UA, day-salt). Day-rotating
    # salt means a visitor can't be tracked across days from the aggregate.
    salt = row[COL["date"]]
    vid = hashlib.sha256(f"{row[COL['ip']]}|{ua}|{salt}".encode()).hexdigest()[:16]
    agg["visitors"].add(vid)

    ref = row[COL["referer"]]
    if ref and ref != "-":
        host = urllib.parse.urlparse(ref).hostname or ""
        # Treat same-site referrers as direct: we only want external sources.
        host = host if host and not host.endswith(SITE_HOSTNAME) else "(direct)"
    else:
        host = "(direct)"
    agg["referrers"][host] += 1

    edge = row[COL["edge"]][:3]
    if edge:
        agg["edges"][edge] += 1


def agg_to_json(agg):
    """Serialise to a JSON-friendly dict. Visitors becomes a count."""
    return {
        "pageviews": agg["pageviews"],
        "bot_requests": agg["bot_requests"],
        "visitors": len(agg["visitors"]) if isinstance(agg["visitors"], set) else agg["visitors"],
        "pages": dict(agg["pages"]),
        "referrers": dict(agg["referrers"]),
        "edges": dict(agg["edges"]),
    }


def merge_aggs(parts):
    """Merge serialised aggregates. Visitors sums approximately (cross-window)."""
    out = empty_aggregate()
    out["visitors"] = 0
    for a in parts:
        out["pageviews"] += a.get("pageviews", 0)
        out["bot_requests"] += a.get("bot_requests", 0)
        out["visitors"] += a.get("visitors", 0)
        out["pages"].update(a.get("pages", {}))
        out["referrers"].update(a.get("referrers", {}))
        out["edges"].update(a.get("edges", {}))
    return out


# --- S3 plumbing ------------------------------------------------------------

def aggregate_raw_logs(s3, start_date, end_date):
    """Read all CF logs whose filename date falls in [start, end] and aggregate."""
    agg = empty_aggregate()
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
                update_aggregate(agg, row)
    return agg


def load_weekly_aggregates(s3, start_date, end_date):
    """Yield JSON aggregates whose ISO week overlaps [start, end]."""
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
            yield json.loads(body)


# --- "Days since last post" nudge ------------------------------------------

def fetch_last_post():
    """Return ('/posts/slug-DDMMYYYY/', days_ago) by scraping the home page."""
    try:
        with urllib.request.urlopen(SITE_URL, timeout=10) as resp:
            html = resp.read().decode("utf-8", errors="replace")
    except Exception:
        return (None, None)
    latest = None
    for m in POST_URL.finditer(html):
        slug, dd, mm, yyyy = m.group(1), m.group(2), m.group(3), m.group(4)
        try:
            d = date(int(yyyy), int(mm), int(dd))
        except ValueError:
            continue
        if latest is None or d > latest[1]:
            latest = (m.group(0), d)
    if not latest:
        return (None, None)
    return (latest[0], (date.today() - latest[1]).days)


# --- Report formatting ------------------------------------------------------

def _delta(cur, prev):
    if not prev:
        return ""
    pct = (cur - prev) / prev * 100
    sign = "+" if pct >= 0 else ""
    return f"  ({sign}{pct:.0f}% vs prior)"


def format_report(cur, prev, last_post, label):
    """Build the plain-text report body."""
    out = [f"tklon.com — {label}", ""]
    permalink, days_since = last_post
    if days_since is not None:
        plural = "" if days_since == 1 else "s"
        out += [f"📝 Days since last post: {days_since} day{plural}",
                f"   Last published: {permalink}", ""]

    out += [
        f"PAGEVIEWS    {cur['pageviews']:>5d}{_delta(cur['pageviews'], (prev or {}).get('pageviews', 0))}",
        f"VISITORS     {cur['visitors']:>5d}   (unique, approx)",
        f"BOT TRAFFIC  {cur['bot_requests']:>5d}   (filtered)",
        "",
    ]

    def top_section(title, counter, decorate=lambda k: k):
        out.append(title)
        for k, n in Counter(counter).most_common(10):
            out.append(f"  {n:>5d}  {decorate(k)}")
        out.append("")

    top_section("TOP PAGES", cur["pages"])
    top_section("TOP REFERRERS", cur["referrers"])
    top_section(
        "TOP EDGE LOCATIONS",
        cur["edges"],
        lambda code: f"{code} ({EDGE_CITY[code]})" if code in EDGE_CITY else code,
    )
    return "\n".join(out).rstrip() + "\n"


# --- Window math ------------------------------------------------------------

WINDOW_RE = re.compile(r"^(\d+)(d|w|mo|y)$")
UNIT_DAYS = {"d": 1, "w": 7, "mo": 30, "y": 365}


def parse_window(s):
    """'7d' → timedelta(7); 'all' → None (open-ended)."""
    if s == "all":
        return None
    m = WINDOW_RE.match(s)
    if not m:
        raise ValueError(f"Invalid window {s!r}: use Nd, Nw, Nmo, Ny, or 'all'")
    return timedelta(days=int(m[1]) * UNIT_DAYS[m[2]])


# --- Entry points -----------------------------------------------------------

def lambda_handler(event, context):
    """EventBridge → Lambda. Last 7d → SNS email + S3 archive."""
    import boto3
    s3 = boto3.client("s3")
    sns = boto3.client("sns")

    end = datetime.now(timezone.utc).date()
    start = end - timedelta(days=7)
    cur = agg_to_json(aggregate_raw_logs(s3, start, end))
    prev = agg_to_json(aggregate_raw_logs(s3, start - timedelta(days=7), start - timedelta(days=1)))

    label = f"week ending {end.isoformat()}"
    body = format_report(cur, prev, fetch_last_post(), label)

    sns.publish(
        TopicArn=SNS_TOPIC_ARN,
        Subject=f"tklon.com — {label}",
        Message=body,
    )

    iso_year, iso_week, _ = end.isocalendar()
    key = f"weekly/{iso_year}-W{iso_week:02d}.json"
    s3.put_object(
        Bucket=STATS_BUCKET, Key=key,
        Body=json.dumps(cur, indent=2).encode(),
        ContentType="application/json",
    )
    return {"status": "ok", "archived": key, "pageviews": cur["pageviews"]}


def cli(argv):
    """`make stats` entry point. Window is parsed, raw logs and/or weekly aggregates are stitched."""
    import boto3
    p = argparse.ArgumentParser()
    p.add_argument("--window", default="7d", help="Nd, Nw, Nmo, Ny, or 'all'")
    p.add_argument("--local", action="store_true", help="Required outside Lambda")
    args = p.parse_args(argv)
    if not args.local:
        p.error("--local is required when running as a CLI")

    s3 = boto3.client("s3")
    window = parse_window(args.window)
    end = datetime.now(timezone.utc).date()
    raw_horizon = end - timedelta(days=90)  # CF logs live 90d

    if window is None or end - window < raw_horizon:
        # Long window: stitch stored aggregates + current-partial-week raw logs.
        start = date(2020, 1, 1) if window is None else end - window
        parts = list(load_weekly_aggregates(s3, start, end))
        parts.append(agg_to_json(aggregate_raw_logs(s3, max(start, raw_horizon), end)))
        cur = agg_to_json(merge_aggs(parts))
        prev = {}  # no comparison for long windows
    else:
        # Short window: raw logs only, with prior-period comparison.
        start = end - window
        cur = agg_to_json(aggregate_raw_logs(s3, start, end))
        prev_end = start - timedelta(days=1)
        prev_start = prev_end - window + timedelta(days=1)
        prev = agg_to_json(aggregate_raw_logs(s3, prev_start, prev_end))

    label = "all time" if window is None else f"last {args.window}"
    print(format_report(cur, prev, fetch_last_post(), label))


if __name__ == "__main__":
    cli(sys.argv[1:])
