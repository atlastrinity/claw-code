#!/usr/bin/env python3
"""Generate and validate the Claw Code 2.0 roadmap board.

The board is intentionally derived from the frozen ROADMAP.md headings so the
validation can prove zero unmapped roadmap headings. Optional .omx research and
plan files are summarized as source metadata without mutating Ultragoal state.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

VALID_STATUSES = {
    "context",
    "active",
    "open",
    "done_verify",
    "stale_done",
    "superseded",
    "deferred_with_rationale",
    "rejected_not_claw",
}
REQUIRED_ITEM_FIELDS = {
    "id",
    "title",
    "source_anchor",
    "source_type",
    "release_bucket",
    "lifecycle_status",
    "dependencies",
    "verification_required",
    "deferral_rationale",
}
OPTIONAL_SOURCES = [
    ".omx/research/claw-open-latest.json",
    ".omx/research/claw-issues.json",
    ".omx/research/codex-repo.json",
    ".omx/research/codex-issues.json",
    ".omx/research/opencode-repo.json",
    ".omx/research/opencode-issues.json",
    ".omx/plans/claw-code-2-0-adaptive-plan.md",
]


@dataclass(frozen=True)
class Heading:
    line: int
    level: int
    title: str
    slug: str
    parent_phase: str | None


def slugify(text: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return slug or "heading"


def sha256(path: Path) -> str | None:
    if not path.exists():
        return None
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def read_headings(roadmap: Path) -> list[Heading]:
    headings: list[Heading] = []
    current_phase: str | None = None
    seen: dict[str, int] = {}
    for line_no, line in enumerate(roadmap.read_text(encoding="utf-8").splitlines(), 1):
        m = re.match(r"^(#{1,6})\s+(.*\S)\s*$", line)
        if not m:
            continue
        level = len(m.group(1))
        title = m.group(2).strip()
        base = slugify(title)
        seen[base] = seen.get(base, 0) + 1
        slug = base if seen[base] == 1 else f"{base}-{seen[base]}"
        parent_phase = current_phase
        if level == 2:
            if title.startswith("Phase "):
                current_phase = title
                parent_phase = current_phase
            else:
                # Top-level buckets after the phase list (Immediate Backlog,
                # Deployment gaps, Pinpoints, etc.) are standalone buckets, not
                # children of the preceding phase.
                current_phase = None
                parent_phase = None
        headings.append(Heading(line_no, level, title, slug, parent_phase))
    return headings


def classify(heading: Heading) -> dict[str, Any]:
    t = heading.title
    lower = t.lower()

    if heading.level == 1 or t in {"Goal", 'Definition of "clawable"', "Current Pain Points", "Product Principles", "Roadmap"}:
        status = "context"
    elif "rejected" in lower or "not claw" in lower:
        status = "rejected_not_claw"
    elif "superseded" in lower or "deprecated" in lower:
        # Deprecated items are still tracked because they can require migration work.
        status = "superseded" if "implemented" in lower or "fixed" in lower else "open"
    elif "deferred" in lower:
        status = "deferred_with_rationale"
    elif "implemented" in lower:
        status = "done_verify"
    elif "fixed" in lower:
        status = "stale_done"
    elif heading.level == 2 and t.startswith("Phase "):
        status = "active"
    else:
        status = "open"

    if heading.level == 1:
        source_type = "roadmap_title"
    elif t.startswith("Phase "):
        source_type = "roadmap_phase"
    elif t.startswith("Pinpoint #"):
        source_type = "roadmap_pinpoint"
    elif heading.level <= 2:
        source_type = "roadmap_context_heading" if status == "context" else "roadmap_backlog_bucket"
    else:
        source_type = "roadmap_item"

    bucket = "context"
    if heading.parent_phase:
        bucket = slugify(heading.parent_phase)
    elif t.startswith("Phase "):
        bucket = slugify(t)
    elif t.startswith("Pinpoint #"):
        bucket = "pinpoints"
    elif "Immediate Backlog" in t:
        bucket = "immediate-backlog"
    elif heading.level == 2 and status != "context":
        bucket = slugify(t)

    deps: list[str] = []
    if heading.parent_phase and heading.level > 2:
        deps.append(slugify(heading.parent_phase))
    if "plugin" in lower or "mcp" in lower:
        deps.append("phase-5-plugin-and-mcp-lifecycle-maturity")
    if "event" in lower or "report" in lower or "schema" in lower:
        deps.append("phase-2-event-native-clawhip-integration")
    if "branch" in lower or "test" in lower or "recovery" in lower:
        deps.append("phase-3-branch-test-awareness-and-auto-recovery")
    if "worker" in lower or "boot" in lower or "startup" in lower:
        deps.append("phase-1-reliable-worker-boot")
    deps = sorted(set(d for d in deps if d != slugify(t)))

    deferral = None
    if status == "deferred_with_rationale":
        deferral = "Roadmap title explicitly marks this item deferred; retain as tracked context until a downstream plan reactivates it."
    elif status == "rejected_not_claw":
        deferral = "Rejected because the roadmap title marks it as not part of the Claw Code product surface."

    return {
        "source_type": source_type,
        "release_bucket": bucket,
        "lifecycle_status": status,
        "dependencies": deps,
        "verification_required": status not in {"context", "rejected_not_claw"},
        "deferral_rationale": deferral,
    }


def source_manifest(repo_root: Path, context_root: Path) -> list[dict[str, Any]]:
    manifest: list[dict[str, Any]] = []
    for rel in ["ROADMAP.md", *OPTIONAL_SOURCES]:
        base = repo_root if rel == "ROADMAP.md" else context_root
        path = base / rel
        entry: dict[str, Any] = {
            "path": rel,
            "exists": path.exists(),
            "sha256": sha256(path),
        }
        if path.exists() and path.suffix == ".json":
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
                entry["record_count"] = len(data) if isinstance(data, list) else len(data) if isinstance(data, dict) else None
            except Exception as exc:  # validation will surface malformed source separately if needed.
                entry["json_error"] = str(exc)
        manifest.append(entry)
    return manifest


def generate(repo_root: Path, context_root: Path) -> dict[str, Any]:
    roadmap = repo_root / "ROADMAP.md"
    headings = read_headings(roadmap)
    items = []
    for index, h in enumerate(headings, 1):
        c = classify(h)
        items.append({
            "id": f"roadmap-{index:03d}-{h.slug}",
            "title": h.title,
            "source_anchor": f"ROADMAP.md:L{h.line}#{h.slug}",
            "source_type": c["source_type"],
            "release_bucket": c["release_bucket"],
            "lifecycle_status": c["lifecycle_status"],
            "dependencies": c["dependencies"],
            "verification_required": c["verification_required"],
            "deferral_rationale": c["deferral_rationale"],
            "roadmap_level": h.level,
            "roadmap_line": h.line,
        })
    status_counts: dict[str, int] = {}
    for item in items:
        status_counts[item["lifecycle_status"]] = status_counts.get(item["lifecycle_status"], 0) + 1
    return {
        "schema_version": "cc2.board.v1",
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "goal_id": "G001-stream0-board",
        "source_policy": "ROADMAP.md headings are canonical; optional research/plan files are recorded in source_manifest and used as context for downstream prioritization, without mutating .omx/ultragoal.",
        "source_manifest": source_manifest(repo_root, context_root),
        "summary": {
            "roadmap_heading_count": len(headings),
            "board_item_count": len(items),
            "lifecycle_status_counts": dict(sorted(status_counts.items())),
        },
        "items": items,
    }


def write_markdown(board: dict[str, Any], path: Path) -> None:
    lines = [
        "# Claw Code 2.0 Canonical Board",
        "",
        f"- Goal: `{board['goal_id']}`",
        f"- Schema: `{board['schema_version']}`",
        f"- Generated: `{board['generated_at']}`",
        f"- ROADMAP headings mapped: `{board['summary']['roadmap_heading_count']}`",
        "",
        "## Source Manifest",
        "",
        "| Source | Exists | SHA-256 | Records |",
        "| --- | --- | --- | ---: |",
    ]
    for src in board["source_manifest"]:
        lines.append(f"| `{src['path']}` | {src['exists']} | `{src['sha256'] or ''}` | {src.get('record_count', '')} |")
    lines.extend([
        "",
        "## Lifecycle Summary",
        "",
        "| Status | Count |",
        "| --- | ---: |",
    ])
    for status, count in board["summary"]["lifecycle_status_counts"].items():
        lines.append(f"| `{status}` | {count} |")
    lines.extend([
        "",
        "## Board Items",
        "",
        "| ID | Source | Type | Bucket | Status | Verify | Dependencies | Deferral |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ])
    for item in board["items"]:
        deps = ", ".join(f"`{d}`" for d in item["dependencies"])
        deferral = item["deferral_rationale"] or ""
        lines.append(
            f"| `{item['id']}` | `{item['source_anchor']}` | `{item['source_type']}` | "
            f"`{item['release_bucket']}` | `{item['lifecycle_status']}` | {item['verification_required']} | {deps} | {deferral} |"
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def validate(repo_root: Path, board_path: Path) -> list[str]:
    errors: list[str] = []
    roadmap = repo_root / "ROADMAP.md"
    headings = read_headings(roadmap)
    try:
        board = json.loads(board_path.read_text(encoding="utf-8"))
    except Exception as exc:
        return [f"failed to read board JSON: {exc}"]
    items = board.get("items")
    if not isinstance(items, list):
        return ["board.items must be a list"]
    expected = {f"ROADMAP.md:L{h.line}#{h.slug}": h.title for h in headings}
    actual = {item.get("source_anchor"): item for item in items if isinstance(item, dict)}
    missing = sorted(set(expected) - set(actual))
    extra = sorted(set(actual) - set(expected))
    if missing:
        errors.append(f"missing ROADMAP heading mappings: {missing[:10]}{' ...' if len(missing) > 10 else ''}")
    if extra:
        errors.append(f"board has non-ROADMAP anchors not in frozen heading set: {extra[:10]}{' ...' if len(extra) > 10 else ''}")
    for anchor, item in actual.items():
        missing_fields = REQUIRED_ITEM_FIELDS - set(item)
        if missing_fields:
            errors.append(f"{anchor}: missing fields {sorted(missing_fields)}")
        status = item.get("lifecycle_status")
        if status not in VALID_STATUSES:
            errors.append(f"{anchor}: invalid lifecycle_status {status!r}")
        if not isinstance(item.get("dependencies"), list):
            errors.append(f"{anchor}: dependencies must be a list")
        if not isinstance(item.get("verification_required"), bool):
            errors.append(f"{anchor}: verification_required must be boolean")
        if status == "deferred_with_rationale" and not item.get("deferral_rationale"):
            errors.append(f"{anchor}: deferred item requires deferral_rationale")
        if item.get("title") != expected.get(anchor):
            errors.append(f"{anchor}: title mismatch board={item.get('title')!r} roadmap={expected.get(anchor)!r}")
    summary = board.get("summary", {})
    if summary.get("roadmap_heading_count") != len(headings):
        errors.append(f"summary roadmap_heading_count mismatch: {summary.get('roadmap_heading_count')} != {len(headings)}")
    if summary.get("board_item_count") != len(items):
        errors.append(f"summary board_item_count mismatch: {summary.get('board_item_count')} != {len(items)}")
    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["generate", "validate"])
    parser.add_argument("--repo-root", default=".", help="repository root containing ROADMAP.md")
    parser.add_argument("--context-root", default=".", help="root containing optional .omx research/plan files")
    parser.add_argument("--board-json", default=".omx/cc2/board.json")
    parser.add_argument("--board-md", default=".omx/cc2/board.md")
    args = parser.parse_args(argv)

    repo_root = Path(args.repo_root).resolve()
    context_root = Path(args.context_root).resolve()
    board_json = repo_root / args.board_json
    board_md = repo_root / args.board_md

    if args.command == "generate":
        board_json.parent.mkdir(parents=True, exist_ok=True)
        board = generate(repo_root, context_root)
        board_json.write_text(json.dumps(board, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        write_markdown(board, board_md)
        print(f"generated {board_json} and {board_md} with {board['summary']['board_item_count']} items")
        return 0

    errors = validate(repo_root, board_json)
    if errors:
        print("CC2 board validation FAILED", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print(f"CC2 board validation PASS: every ROADMAP heading is mapped in {board_json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
