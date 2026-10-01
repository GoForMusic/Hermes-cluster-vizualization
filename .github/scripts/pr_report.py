#!/usr/bin/env python3
"""Test report + code coverage of one CI job, as one comment on the pull request (created the first time, updated after that).

    pr_report.py --key hub --title "Hub (Rust)" --junit target/nextest/ci/junit.xml --coverage cov.json --coverage-format llvm
    pr_report.py --key agent-windows --title "Windows agent (Rust)" --junit target/nextest/ci/junit.xml     # no --coverage: the tests only see what compiles here
    pr_report.py --key web --title "Web (TypeScript)" --junit junit.xml --coverage coverage/coverage-summary.json --coverage-format vitest

JUnit comes from cargo-nextest and from vitest; coverage from `cargo llvm-cov --json --summary-only` (llvm) or vitest's json-summary (vitest).
The comment is found again by a hidden marker with --key, so every job keeps its own comment and the jobs never overwrite each other.
Needs GITEA_TOKEN (the job's token is enough) and the usual GITHUB_* variables; without them, or outside a pull request, it only prints the report.
Exit code is always 0: a report that could not be posted must not fail the build.
"""

import argparse
import json
import os
import sys
import urllib.request
import xml.etree.ElementTree as ET

WORST_FILES = 10
FAILURES_SHOWN = 20


def read_junit(paths):
    total = failed = skipped = 0
    seconds = 0.0
    failures = []
    for path in paths:
        if not os.path.exists(path):
            continue
        root = ET.parse(path).getroot()
        # the suites' own time is the wall clock; the cases' times add up across tests that ran in parallel
        cases = list(root.iter("testcase"))
        seconds += float(root.get("time") or sum(float(c.get("time") or 0) for c in cases))
        for case in cases:
            total += 1
            bad = case.find("failure")
            if bad is None:
                bad = case.find("error")
            if bad is not None:
                failed += 1
                name = f'{case.get("classname") or ""} {case.get("name") or ""}'.strip()
                failures.append((name, (bad.get("message") or bad.text or "").strip()))
            elif case.find("skipped") is not None:
                skipped += 1
    return {"total": total, "failed": failed, "skipped": skipped, "passed": total - failed - skipped, "seconds": seconds, "failures": failures}


def relative(name):
    root = os.environ.get("GITHUB_WORKSPACE") or os.getcwd()
    return name[len(root) + 1:] if name.startswith(root + os.sep) else name


def read_coverage(path, fmt):
    """-> ({metric: (covered, total)}, [(file, covered, total)]) for lines, functions and (regions | branches)"""
    if not path or not os.path.exists(path):
        return None
    with open(path) as f:
        doc = json.load(f)
    files = []
    if fmt == "llvm":
        data = doc["data"][0]
        pick = {"lines": "lines", "functions": "functions", "regions": "regions"}
        totals = {label: (data["totals"][key]["covered"], data["totals"][key]["count"]) for label, key in pick.items()}
        for entry in data["files"]:
            lines = entry["summary"]["lines"]
            files.append((relative(entry["filename"]), lines["covered"], lines["count"]))
    else:
        pick = {"lines": "lines", "functions": "functions", "branches": "branches"}
        totals = {label: (doc["total"][key]["covered"], doc["total"][key]["total"]) for label, key in pick.items()}
        for name, entry in doc.items():
            if name != "total":
                files.append((relative(name), entry["lines"]["covered"], entry["lines"]["total"]))
    return totals, files


def pct(covered, total):
    return 100.0 if total == 0 else 100.0 * covered / total


def render(key, title, tests, coverage, wanted):
    out = [f"<!-- ci-report:{key} -->", f"### {title}", ""]
    icon = "✅" if tests["total"] and not tests["failed"] else "❌"
    out += [
        "| | Tests | Passed | Failed | Skipped | Time |",
        "|---|---:|---:|---:|---:|---:|",
        f'| {icon} | {tests["total"]} | {tests["passed"]} | {tests["failed"]} | {tests["skipped"]} | {tests["seconds"]:.1f}s |',
        "",
    ]
    if not tests["total"]:
        out += ["_No test report was produced: the tests did not run to the end._", ""]
    if tests["failures"]:
        out += [f'<details open><summary>💥 {tests["failed"]} failed</summary>', ""]
        for name, message in tests["failures"][:FAILURES_SHOWN]:
            first = message.splitlines()[0] if message else ""
            out.append(f"- `{name}`" + (f": {first[:200]}" if first else ""))
        if len(tests["failures"]) > FAILURES_SHOWN:
            out.append(f'- … and {len(tests["failures"]) - FAILURES_SHOWN} more')
        out += ["", "</details>", ""]
    if coverage:
        totals, files = coverage
        out += ["| Coverage | Covered | Total | |", "|---|---:|---:|---:|"]
        for label, (covered, total) in totals.items():
            out.append(f"| {label} | {covered} | {total} | **{pct(covered, total):.1f}%** |")
        out.append("")
        worst = sorted((f for f in files if f[2] > 0), key=lambda f: (pct(f[1], f[2]), -f[2]))[:WORST_FILES]
        if worst:
            out += [f"<details><summary>🔻 Least covered files (lines)</summary>", "", "| File | Lines | |", "|---|---:|---:|"]
            out += [f"| `{name}` | {covered}/{total} | {pct(covered, total):.1f}% |" for name, covered, total in worst]
            out += ["", "</details>", ""]
    elif wanted:
        out += ["_No coverage report was produced._", ""]
    sha = os.environ.get("GITHUB_SHA", "")[:8]
    if sha:
        out.append(f"<sub>commit {sha}</sub>")
    return "\n".join(out) + "\n"


def pull_request_number():
    path = os.environ.get("GITHUB_EVENT_PATH")
    if not path or not os.path.exists(path):
        return None
    with open(path) as f:
        event = json.load(f)
    return (event.get("pull_request") or {}).get("number")


def api(method, url, token, body=None):
    request = urllib.request.Request(
        url,
        method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Authorization": f"token {token}", "Content-Type": "application/json", "Accept": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def post(markdown, key):
    # GITEA_TOKEN on the Gitea pipelines, GITHUB_TOKEN (the job's built-in token, no secret to configure) on GitHub's.
    token = os.environ.get("GITEA_TOKEN") or os.environ.get("GITHUB_TOKEN")
    server = os.environ.get("GITHUB_SERVER_URL")
    repo = os.environ.get("GITHUB_REPOSITORY")
    number = pull_request_number()
    if not (token and server and repo and number):
        print("not a pull request run, or no token: the report is only printed", file=sys.stderr)
        return
    # real GitHub serves its REST API from a different host than the site itself; a Gitea instance serves both from the same one.
    base = "https://api.github.com" if server.rstrip("/") == "https://github.com" else f"{server.rstrip('/')}/api/v1"
    base = f"{base}/repos/{repo}"
    page_size = "per_page" if "api.github.com" in base else "limit"
    marker = f"<!-- ci-report:{key} -->"
    existing = None
    page = 1
    while existing is None:
        comments = api("GET", f"{base}/issues/{number}/comments?page={page}&{page_size}=50", token)
        existing = next((c for c in comments if marker in (c.get("body") or "")), None)
        if len(comments) < 50:
            break
        page += 1
    if existing:
        api("PATCH", f"{base}/issues/comments/{existing['id']}", token, {"body": markdown})
        print(f"updated comment {existing['id']} on #{number}")
    else:
        created = api("POST", f"{base}/issues/{number}/comments", token, {"body": markdown})
        print(f"created comment {created['id']} on #{number}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--key", required=True)
    parser.add_argument("--title", required=True)
    parser.add_argument("--junit", action="append", default=[])
    parser.add_argument("--coverage")
    parser.add_argument("--coverage-format", choices=["llvm", "vitest"], default="llvm")
    args = parser.parse_args()

    markdown = render(args.key, args.title, read_junit(args.junit), read_coverage(args.coverage, args.coverage_format), bool(args.coverage))
    print(markdown)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write(markdown)
    try:
        post(markdown, args.key)
    except Exception as e:  # the report is a courtesy: never fail the job over it
        print(f"could not post the report: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
